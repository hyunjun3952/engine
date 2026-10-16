use std::cmp::Reverse;
use std::collections::BinaryHeap;

const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;

/// The cheapest route over a `w` by `h` grid from `start` to `goal`, both ends included, moving to the four
/// neighbours (and, with `diagonal`, the four corners, but never cutting past a blocked cell). A straight step costs
/// 10 and a diagonal 14. `None` when `goal` is blocked or not reachable. A blocked `start` is allowed.
pub(crate) fn astar(
    (w, h): (usize, usize),
    start: (usize, usize),
    goal: (usize, usize),
    diagonal: bool,
    mut open: impl FnMut(usize, usize) -> bool,
) -> Option<Vec<(usize, usize)>> {
    if start.0 >= w || start.1 >= h || goal.0 >= w || goal.1 >= h || !open(goal.0, goal.1) {
        return None;
    }
    let cell = |x: usize, y: usize| y * w + x;
    // Octile distance when diagonals are allowed, Manhattan otherwise: never more than the real cost left.
    let guess = |x: usize, y: usize| {
        let (dx, dy) = (x.abs_diff(goal.0) as u32, y.abs_diff(goal.1) as u32);
        if diagonal { STRAIGHT * (dx + dy) - (2 * STRAIGHT - DIAGONAL) * dx.min(dy) } else { STRAIGHT * (dx + dy) }
    };
    let mut cost = vec![u32::MAX; w * h];
    let mut from = vec![usize::MAX; w * h];
    let mut queue = BinaryHeap::new();
    cost[cell(start.0, start.1)] = 0;
    queue.push(Reverse((guess(start.0, start.1), 0u32, start.0, start.1)));
    while let Some(Reverse((_, spent, x, y))) = queue.pop() {
        if (x, y) == goal {
            let mut path = vec![(x, y)];
            let mut at = cell(x, y);
            while from[at] != usize::MAX {
                at = from[at];
                path.push((at % w, at / w));
            }
            path.reverse();
            return Some(path);
        }
        if spent > cost[cell(x, y)] {
            continue;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let turned = dx != 0 && dy != 0;
            if turned && !diagonal {
                continue;
            }
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !open(nx, ny) || (turned && !(open(nx, y) && open(x, ny))) {
                continue;
            }
            let next = spent + if turned { DIAGONAL } else { STRAIGHT };
            if next < cost[cell(nx, ny)] {
                cost[cell(nx, ny)] = next;
                from[cell(nx, ny)] = cell(x, y);
                queue.push(Reverse((next + guess(nx, ny), next, nx, ny)));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> impl Fn(usize, usize) -> bool {
        let rows: Vec<Vec<u8>> = rows.iter().map(|r| r.bytes().collect()).collect();
        move |x, y| rows[y][x] != b'#'
    }

    #[test]
    fn it_finds_the_shortest_way_round_a_wall() {
        let open = grid(&["....", ".##.", "...."]);
        let path = astar((4, 3), (0, 0), (3, 2), false, open).unwrap();
        assert_eq!((path.first(), path.last(), path.len()), (Some(&(0, 0)), Some(&(3, 2)), 6));
        assert!(path.windows(2).all(|p| p[0].0.abs_diff(p[1].0) + p[0].1.abs_diff(p[1].1) == 1), "{path:?}");
    }

    #[test]
    fn diagonals_are_shorter_and_never_cut_a_corner() {
        let open = grid(&["...", "...", "..."]);
        assert_eq!(astar((3, 3), (0, 0), (2, 2), true, &open).unwrap(), [(0, 0), (1, 1), (2, 2)]);
        assert_eq!(astar((3, 3), (0, 0), (2, 2), false, &open).unwrap().len(), 5);
        let tight = grid(&[".#", "#."]);
        assert!(astar((2, 2), (0, 0), (1, 1), true, tight).is_none(), "a diagonal between two walls is not a way");
    }

    #[test]
    fn it_gives_up_on_a_blocked_or_unreachable_goal_and_handles_trivial_cases() {
        let open = grid(&[".#.", ".#.", ".#."]);
        assert!(astar((3, 3), (0, 0), (2, 0), true, &open).is_none(), "a wall splits the grid");
        assert!(astar((3, 3), (0, 0), (1, 1), true, &open).is_none(), "the goal is a wall");
        assert!(astar((3, 3), (0, 0), (5, 5), true, &open).is_none(), "outside the grid");
        assert_eq!(astar((3, 3), (0, 1), (0, 1), true, &open).unwrap(), [(0, 1)]);
        assert_eq!(astar((3, 3), (1, 1), (0, 1), true, &open).unwrap(), [(1, 1), (0, 1)], "a blocked start may leave");
    }

    #[test]
    fn it_matches_a_breadth_first_search_on_random_grids() {
        let mut rng = crate::Rng::new(5);
        for case in 0..200 {
            let (w, h) = (3 + case % 8, 3 + case % 6);
            let walls: Vec<bool> = (0..w * h).map(|_| rng.f32() < 0.3).collect();
            let open = |x: usize, y: usize| !walls[y * w + x];
            let (start, goal) = ((0, 0), (w - 1, h - 1));
            let mut dist = vec![usize::MAX; w * h];
            let mut queue = std::collections::VecDeque::from([start]);
            dist[0] = 0;
            while let Some((x, y)) = queue.pop_front() {
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let (nx, ny) = (nx as usize, ny as usize);
                        if open(nx, ny) && dist[ny * w + nx] == usize::MAX {
                            dist[ny * w + nx] = dist[y * w + x] + 1;
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
            let found = astar((w, h), start, goal, false, open);
            let want = open(goal.0, goal.1).then(|| dist[goal.1 * w + goal.0]).filter(|&d| d != usize::MAX);
            assert_eq!(found.map(|p| p.len() - 1), want, "case {case}");
        }
    }
}
