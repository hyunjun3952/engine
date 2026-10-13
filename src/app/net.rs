use super::input::Input;
use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};

// Frames in one packet; a peer further behind than this catches up over several packets.
const MAX_FRAMES: usize = 64;
// How far past the tick being waited on (and the input delay) the local player may queue frames.
const MAX_AHEAD: u32 = 30;
// Checksums carried by a packet, the latest ones.
const SUMS: usize = 4;
// Ticks of checksums kept, so a peer that reports late is still compared.
const KEEP_SUMS: u32 = 256;
const MAGIC: u8 = 0xE7;

/// The actions of a game in a fixed order, so a step's held actions fit one `u64` that every peer reads the same way
/// whatever keys it has bound. At most 64, and every peer must build the list with the same names in the same order.
pub struct Actions {
    names: Vec<String>,
}

impl Actions {
    pub fn new(names: &[&str]) -> Self {
        assert!(names.len() <= 64, "at most 64 networked actions");
        let mut seen = std::collections::BTreeSet::new();
        assert!(names.iter().all(|n| seen.insert(*n)), "an action is listed twice");
        Actions { names: names.iter().map(|n| n.to_string()).collect() }
    }

    /// The actions held on this step of the local `Input`, as the bits to give `Lockstep::push_local`. Panics on an
    /// action that is not bound, as `Input::down` does.
    pub fn mask(&self, input: &Input) -> u64 {
        self.names.iter().enumerate().filter(|(_, n)| input.down(n)).fold(0, |m, (i, _)| m | 1 << i)
    }

    /// One player's actions in a step, from the `now` and `before` bits of `StepInput`.
    pub fn held(&self, now: u64, before: u64) -> Held<'_> {
        Held { actions: self, now, before }
    }
}

/// What one player is holding in a step, read by action name like `Input`.
pub struct Held<'a> {
    actions: &'a Actions,
    now: u64,
    before: u64,
}

impl Held<'_> {
    fn bit(&self, name: &str) -> u64 {
        let i = self.actions.names.iter().position(|n| n == name);
        1 << i.unwrap_or_else(|| panic!("action {name:?} is not in the networked list"))
    }

    pub fn down(&self, name: &str) -> bool {
        self.now & self.bit(name) != 0
    }

    pub fn pressed(&self, name: &str) -> bool {
        self.now & !self.before & self.bit(name) != 0
    }

    pub fn released(&self, name: &str) -> bool {
        self.before & !self.now & self.bit(name) != 0
    }

    /// -1, 0 or 1 from a pair of actions.
    pub fn axis(&self, negative: &str, positive: &str) -> f32 {
        self.down(positive) as i32 as f32 - self.down(negative) as i32 as f32
    }
}

/// Everyone's input for one step, by player index.
#[derive(Clone, Debug, PartialEq)]
pub struct StepInput {
    pub now: Vec<u64>,
    pub before: Vec<u64>,
}

/// One datagram from a peer: the input frames it thinks we still lack, what it still lacks of ours, and its latest
/// state checksums.
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub from: u8,
    // The first tick of ours the sender does not have yet.
    pub ack: u32,
    // The tick of the first of `frames`.
    pub first: u32,
    pub frames: Vec<u64>,
    pub sums: Vec<(u32, u32)>,
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![MAGIC, self.from];
        out.extend(self.ack.to_le_bytes());
        out.extend(self.first.to_le_bytes());
        out.push(self.frames.len() as u8);
        self.frames.iter().for_each(|f| out.extend(f.to_le_bytes()));
        out.push(self.sums.len() as u8);
        self.sums.iter().for_each(|(t, s)| {
            out.extend(t.to_le_bytes());
            out.extend(s.to_le_bytes());
        });
        out
    }

    /// `None` for anything that is not exactly a packet.
    pub fn decode(bytes: &[u8]) -> Option<Packet> {
        let (&magic, rest) = bytes.split_first()?;
        let (&from, rest) = rest.split_first()?;
        let word = |b: &[u8], at: usize| Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?));
        let (ack, first) = (word(rest, 0)?, word(rest, 4)?);
        let (&n, mut rest) = rest.get(8..)?.split_first()?;
        let n = n as usize;
        if magic != MAGIC || n > MAX_FRAMES {
            return None;
        }
        let mut frames = Vec::with_capacity(n);
        for _ in 0..n {
            frames.push(u64::from_le_bytes(rest.get(..8)?.try_into().ok()?));
            rest = &rest[8..];
        }
        let (&m, mut rest) = rest.split_first()?;
        let m = m as usize;
        if m > SUMS {
            return None;
        }
        let mut sums = Vec::with_capacity(m);
        for _ in 0..m {
            sums.push((word(rest, 0)?, word(rest, 4)?));
            rest = &rest[8..];
        }
        rest.is_empty().then_some(Packet { from, ack, first, frames, sums })
    }
}

/// Lockstep for `players` peers over a network that loses and reorders datagrams: every peer runs every step once it
/// has everyone's input for it, so all of them compute the same game.
///
/// Each local input is applied `delay` steps after it is read, which hides the trip time; the first `delay` steps run
/// with no input at all. Each frame goes into every packet until the peer acknowledges it, so a lost packet is covered
/// by the next one and nothing is retransmitted on a timer.
///
/// The game must be a function of the steps' inputs alone: the same start state, a fixed `dt`, its own `Rng` with an
/// agreed seed, no clock. Floating point maths that differs between machines (a different libm) will diverge; the
/// checksums given to `advance` show it as `desync`.
pub struct Lockstep {
    me: usize,
    delay: u32,
    next: u32,
    pushed: u32,
    frames: Vec<BTreeMap<u32, u64>>,
    have: Vec<u32>,
    acked: Vec<u32>,
    before: Vec<u64>,
    mine: BTreeMap<u32, u32>,
    theirs: Vec<BTreeMap<u32, u32>>,
    desync: Option<u32>,
}

impl Lockstep {
    /// Player `me` of `players`, with the input applied `delay` steps after it is read (3 is 50 ms at 60 steps a
    /// second).
    pub fn new(me: usize, players: usize, delay: u32) -> Self {
        assert!(players > 0 && me < players, "player {me} of {players}");
        let blank: BTreeMap<u32, u64> = (0..delay).map(|t| (t, 0)).collect();
        Lockstep {
            me,
            delay,
            next: 0,
            pushed: delay,
            frames: vec![blank; players],
            have: vec![delay; players],
            acked: vec![delay; players],
            before: vec![0; players],
            mine: BTreeMap::new(),
            theirs: vec![BTreeMap::new(); players],
            desync: None,
        }
    }

    /// The step the game runs next.
    pub fn tick(&self) -> u32 {
        self.next
    }

    /// Queues the local player's actions as read now, for the step `delay` ahead; false (and nothing queued) when
    /// the peers are so far behind that the queue is full.
    pub fn push_local(&mut self, held: u64) -> bool {
        if self.pushed - self.next >= self.delay + MAX_AHEAD {
            return false;
        }
        self.frames[self.me].insert(self.pushed, held);
        self.pushed += 1;
        self.have[self.me] = self.pushed;
        true
    }

    /// The datagram for `peer`.
    pub fn packet_for(&self, peer: usize) -> Packet {
        let first = self.acked[peer].min(self.pushed);
        let n = ((self.pushed - first) as usize).min(MAX_FRAMES);
        let frames = (first..first + n as u32).map(|t| self.frames[self.me].get(&t).copied().unwrap_or(0)).collect();
        let sums = self.mine.iter().rev().take(SUMS).map(|(&t, &s)| (t, s)).collect();
        Packet { from: self.me as u8, ack: self.have[peer], first, frames, sums }
    }

    /// Takes in a datagram. One from an unknown player, from ourselves, or for a time far off is ignored.
    pub fn receive(&mut self, packet: &Packet) {
        let from = packet.from as usize;
        let wraps = packet.first.checked_add(packet.frames.len() as u32).is_none();
        if from >= self.frames.len() || from == self.me || wraps {
            return;
        }
        self.acked[from] = self.acked[from].max(packet.ack.min(self.pushed));
        let limit = self.next + self.delay + 4 * MAX_AHEAD + MAX_FRAMES as u32;
        for (i, &frame) in packet.frames.iter().enumerate() {
            let tick = packet.first + i as u32;
            if tick >= self.next && tick < limit {
                self.frames[from].entry(tick).or_insert(frame);
            }
        }
        while self.frames[from].contains_key(&self.have[from]) {
            self.have[from] += 1;
        }
        for &(tick, sum) in &packet.sums {
            match self.mine.get(&tick) {
                Some(&own) if own != sum => self.desync = Some(self.desync.map_or(tick, |d| d.min(tick))),
                Some(_) => {}
                None if tick >= self.next && tick < limit => {
                    self.theirs[from].insert(tick, sum);
                }
                None => {}
            }
        }
    }

    /// Everyone's input for `tick()` once all of it has arrived.
    pub fn ready(&self) -> Option<StepInput> {
        let now: Option<Vec<u64>> = self.frames.iter().map(|f| f.get(&self.next).copied()).collect();
        now.map(|now| StepInput { now, before: self.before.clone() })
    }

    /// The players whose input for `tick()` has not come, for a "waiting for player 2" message.
    pub fn waiting_on(&self) -> Vec<usize> {
        (0..self.frames.len()).filter(|&p| !self.frames[p].contains_key(&self.next)).collect()
    }

    /// The game ran the step: moves on to the next. `sum` is a checksum of the game's state after the step, if it
    /// made one (every few steps is plenty); peers that report a different one for the same step set `desync`.
    pub fn advance(&mut self, sum: Option<u32>) {
        let tick = self.next;
        for (p, frames) in self.frames.iter().enumerate() {
            self.before[p] = frames.get(&tick).copied().unwrap_or(0);
        }
        if let Some(sum) = sum {
            self.mine.insert(tick, sum);
            if self.theirs.iter().any(|t| t.get(&tick).is_some_and(|&s| s != sum)) {
                self.desync = Some(self.desync.map_or(tick, |d| d.min(tick)));
            }
        }
        self.next += 1;
        let others = (0..self.frames.len()).filter(|&p| p != self.me).map(|p| self.acked[p]).min();
        let keep_mine = others.map_or(self.next, |a| a.min(self.next));
        for (p, frames) in self.frames.iter_mut().enumerate() {
            let keep = if p == self.me { keep_mine } else { self.next };
            *frames = frames.split_off(&keep);
        }
        let old = self.next.saturating_sub(KEEP_SUMS);
        self.mine = self.mine.split_off(&old);
        self.theirs.iter_mut().for_each(|t| *t = t.split_off(&self.next.saturating_sub(1)));
    }

    /// The first step at which a peer's state differed from ours, once one has been seen.
    pub fn desync(&self) -> Option<u32> {
        self.desync
    }
}

/// UDP for a `Lockstep`: `send` once per game tick and `recv` before asking whether the step is `ready`. Players are
/// numbered as in the address list given to `bind`.
pub struct Udp {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
    me: usize,
}

impl Udp {
    /// Listens on `addr` as player `me`; `peers[i]` is where player `i` listens (the entry for `me` is unused).
    pub fn bind(addr: &str, me: usize, peers: &[SocketAddr]) -> std::io::Result<Udp> {
        assert!(me < peers.len(), "player {me} of {}", peers.len());
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Udp { socket, peers: peers.to_vec(), me })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Sends each peer its packet. A failed send is dropped: the next packet carries the same frames.
    pub fn send(&self, lock: &Lockstep) {
        for (p, addr) in self.peers.iter().enumerate().filter(|(p, _)| *p != self.me) {
            let _ = self.socket.send_to(&lock.packet_for(p).encode(), addr);
        }
    }

    /// Takes in everything waiting. Packets that are not from the address of the player they name, or are not
    /// packets, are ignored.
    pub fn recv(&self, lock: &mut Lockstep) {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            if let Some(packet) = Packet::decode(&buf[..n]) {
                if self.peers.get(packet.from as usize) == Some(&from) {
                    lock.receive(&packet);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Body, Entity, Rect, Rng, Vel, World, WorldFile};

    const LEFT: u64 = 1;
    const RIGHT: u64 = 2;

    // The whole game: each player's square moves with its held actions.
    fn game(players: usize) -> (World, Vec<Entity>, WorldFile) {
        let mut w = World::new();
        let ents = (0..players)
            .map(|p| w.spawn((Body(Rect { x: p as f32 * 50.0, y: 0.0, w: 8.0, h: 8.0 }), Vel::default())))
            .collect();
        (w, ents, WorldFile::new().component::<Body>("body").component::<Vel>("vel"))
    }

    fn run_step(w: &mut World, ents: &[Entity], input: &StepInput) {
        for (p, &e) in ents.iter().enumerate() {
            let held = input.now[p];
            let dx = (held & RIGHT != 0) as i32 as f32 - (held & LEFT != 0) as i32 as f32;
            let mut vel = w.get::<&mut Vel>(e).unwrap();
            vel.x = dx * 1.5 + vel.x * 0.5;
            let vx = vel.x;
            drop(vel);
            w.get::<&mut Body>(e).unwrap().0.x += vx;
        }
    }

    // What player `p` presses at its engine tick `t`.
    fn pressed(p: usize, t: u32) -> u64 {
        match (t / (7 + 3 * p as u32)) % 3 {
            0 => LEFT,
            1 => RIGHT,
            _ => 0,
        }
    }

    // Two peers over a link that drops, delays and reorders packets, run until both reach `target` steps.
    fn play(
        players: usize,
        target: u32,
        loss: f32,
        seed: u32,
        corrupt_at: Option<(usize, u32)>,
    ) -> Vec<(u32, u32, Option<u32>)> {
        let mut rng = Rng::new(seed);
        let mut locks: Vec<Lockstep> = (0..players).map(|p| Lockstep::new(p, players, 3)).collect();
        let mut sims: Vec<_> = (0..players).map(|_| game(players)).collect();
        // (arrives at engine tick, to, packet)
        let mut wire: Vec<(u32, usize, Packet)> = Vec::new();
        let mut local_tick = vec![0u32; players];
        for now in 0..4000u32 {
            for p in 0..players {
                if locks[p].push_local(pressed(p, local_tick[p])) {
                    local_tick[p] += 1;
                }
                for q in (0..players).filter(|&q| q != p) {
                    if rng.range(0.0, 1.0) >= loss {
                        wire.push((now + rng.range(0.0, 4.0) as u32, q, locks[p].packet_for(q)));
                    }
                }
            }
            let (arrived, later): (Vec<_>, Vec<_>) = wire.drain(..).partition(|w| w.0 <= now);
            wire = later;
            for (_, to, packet) in arrived {
                locks[to].receive(&packet);
            }
            for p in 0..players {
                while locks[p].tick() < target {
                    let Some(input) = locks[p].ready() else { break };
                    let (w, ents, file) = &mut sims[p];
                    run_step(w, ents, &input);
                    if corrupt_at == Some((p, locks[p].tick())) {
                        w.get::<&mut Body>(ents[0]).unwrap().0.y += 1.0;
                    }
                    let tick = locks[p].tick();
                    locks[p].advance((tick % 5 == 4).then(|| file.checksum(w)));
                    if rng.range(0.0, 1.0) < 0.5 {
                        break;
                    }
                }
            }
            if locks.iter().all(|l| l.tick() >= target) {
                break;
            }
        }
        (0..players).map(|p| (locks[p].tick(), sims[p].2.checksum(&sims[p].0), locks[p].desync())).collect()
    }

    #[test]
    fn the_first_steps_run_with_blank_input_before_any_packet_arrives() {
        let mut l = Lockstep::new(0, 2, 3);
        for t in 0..3 {
            assert_eq!(l.ready().unwrap().now, vec![0, 0], "tick {t} is blank for both");
            l.advance(None);
        }
        assert!(l.ready().is_none(), "the 4th step needs real frames");
        assert_eq!(l.waiting_on(), vec![0, 1]);
        l.push_local(RIGHT);
        assert_eq!(l.waiting_on(), vec![1], "our own frame is there, the peer's is not");
        let mut solo = Lockstep::new(0, 1, 3);
        (0..3).for_each(|_| solo.advance(None));
        solo.push_local(RIGHT);
        assert_eq!(solo.ready().unwrap().now, vec![RIGHT]);
    }

    #[test]
    fn two_peers_over_a_lossy_reordering_link_reach_the_same_state() {
        for seed in 1..=5 {
            let out = play(2, 300, 0.4, seed, None);
            assert!(out.iter().all(|o| o.0 >= 300), "seed {seed}: both got there: {out:?}");
            assert_eq!(out[0].1, out[1].1, "seed {seed}: same world");
            assert!(out.iter().all(|o| o.2.is_none()), "seed {seed}: no desync reported: {out:?}");
        }
    }

    #[test]
    fn three_peers_agree_too() {
        let out = play(3, 200, 0.3, 9, None);
        assert!(out.iter().all(|o| o.0 >= 200 && o.2.is_none()), "{out:?}");
        assert!(out[0].1 == out[1].1 && out[1].1 == out[2].1);
    }

    #[test]
    fn a_state_that_drifts_is_reported_as_a_desync_at_or_before_the_checksum_step() {
        let out = play(2, 120, 0.1, 4, Some((1, 33)));
        assert_ne!(out[0].1, out[1].1, "the worlds really differ");
        let seen: Vec<u32> = out.iter().filter_map(|o| o.2).collect();
        assert!(!seen.is_empty(), "somebody noticed: {out:?}");
        assert!(seen.iter().all(|&t| (33..=34).contains(&t)), "at the first checksum after the drift: {seen:?}");
    }

    #[test]
    fn a_silent_peer_stalls_the_game_and_the_queue_is_bounded() {
        let mut l = Lockstep::new(0, 2, 2);
        for _ in 0..2 {
            l.advance(None);
        }
        assert!(l.ready().is_none());
        let queued = (0..1000).filter(|_| l.push_local(0)).count();
        assert_eq!(queued as u32, 30 + 2, "no more than the delay and the ahead limit are queued: {queued}");
        assert_eq!(l.waiting_on(), vec![1], "our own frame is there, the silent peer's is not");
        assert!(l.packet_for(1).frames.len() <= 64);
    }

    #[test]
    fn packets_round_trip_and_garbage_is_refused() {
        let p = Packet { from: 1, ack: 9, first: 4, frames: vec![1, u64::MAX, 0], sums: vec![(5, 7), (4, 1)] };
        assert_eq!(Packet::decode(&p.encode()), Some(p.clone()));
        let bytes = p.encode();
        for cut in 0..bytes.len() {
            assert!(Packet::decode(&bytes[..cut]).is_none(), "a packet cut at {cut} is refused");
        }
        let mut long = bytes.clone();
        long.push(0);
        assert!(Packet::decode(&long).is_none(), "trailing bytes are refused");
        let mut wrong = bytes;
        wrong[0] ^= 1;
        assert!(Packet::decode(&wrong).is_none());
        let mut rng = Rng::new(3);
        for _ in 0..5000 {
            let junk: Vec<u8> = (0..rng.range(0.0, 200.0) as usize).map(|_| rng.range(0.0, 256.0) as u8).collect();
            let _ = Packet::decode(&junk);
        }
    }

    #[test]
    fn hostile_packets_change_nothing_they_should_not() {
        let mut l = Lockstep::new(0, 2, 2);
        let junk = |from, ack, first| Packet { from, ack, first, frames: vec![5; 64], sums: vec![(first, 1)] };
        l.receive(&junk(7, 0, 0));
        l.receive(&junk(0, 0, 0));
        l.receive(&junk(1, u32::MAX, u32::MAX - 10));
        l.receive(&junk(1, 0, 1_000_000));
        assert_eq!(l.have[1], 2, "nothing far off or wrapped was taken in");
        assert!(l.frames[1].len() <= 2 + 200, "memory stays bounded");
        assert!(l.desync().is_none());
        assert!(l.packet_for(1).first <= l.pushed, "an absurd ack does not push our window past what we have");
    }

    #[test]
    fn held_actions_read_by_name_and_edges_and_ignore_the_peers_key_bindings() {
        let actions = Actions::new(&["left", "right", "jump"]);
        let mut input = crate::app::input::Input::default();
        input.bind("left", &[crate::Key::Z]);
        input.bind("right", &[crate::Key::X]);
        input.bind("jump", &[crate::Key::C]);
        input.hold(&[crate::Key::X, crate::Key::C]);
        let mask = actions.mask(&input);
        assert_eq!(mask, 0b110);
        let held = actions.held(mask, 0b100);
        assert!(held.down("right") && held.pressed("right") && held.down("jump") && !held.pressed("jump"));
        assert!(!held.down("left") && held.axis("left", "right") == 1.0);
        assert!(actions.held(0, 0b100).released("jump"));
    }

    #[test]
    #[should_panic(expected = "not in the networked list")]
    fn an_action_outside_the_list_panics() {
        Actions::new(&["a"]).held(0, 0).down("b");
    }

    #[test]
    fn two_real_sockets_on_loopback_play_the_same_game() {
        let addr = |u: &Udp| u.local_addr().unwrap();
        let start = |me| Udp::bind("127.0.0.1:0", me, &["127.0.0.1:1".parse().unwrap(); 2]).unwrap();
        let (a0, b0) = (start(0), start(1));
        let peers = [addr(&a0), addr(&b0)];
        let (a, b) = (Udp { peers: peers.to_vec(), ..a0 }, Udp { peers: peers.to_vec(), ..b0 });
        let (mut la, mut lb) = (Lockstep::new(0, 2, 2), Lockstep::new(1, 2, 2));
        let mut sims = [game(2), game(2)];
        let start = std::time::Instant::now();
        let target = 120;
        let mut local = [0u32; 2];
        while (la.tick() < target || lb.tick() < target) && start.elapsed().as_secs() < 5 {
            for (p, lock) in [&mut la, &mut lb].into_iter().enumerate() {
                if lock.push_local(pressed(p, local[p])) {
                    local[p] += 1;
                }
            }
            a.send(&la);
            b.send(&lb);
            std::thread::sleep(std::time::Duration::from_micros(300));
            a.recv(&mut la);
            b.recv(&mut lb);
            for (p, lock) in [&mut la, &mut lb].into_iter().enumerate() {
                while lock.tick() < target {
                    let Some(input) = lock.ready() else { break };
                    let (w, ents, file) = &mut sims[p];
                    run_step(w, ents, &input);
                    let tick = lock.tick();
                    lock.advance((tick % 5 == 4).then(|| file.checksum(w)));
                }
            }
        }
        assert!(la.tick() >= target && lb.tick() >= target, "both finished: {} {}", la.tick(), lb.tick());
        assert_eq!(sims[0].2.checksum(&sims[0].0), sims[1].2.checksum(&sims[1].0));
        assert!(la.desync().is_none() && lb.desync().is_none());
    }
}
