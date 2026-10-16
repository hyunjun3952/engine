#[derive(Debug)]
pub(super) struct Display(pub(super) wgpu::rwh::RawDisplayHandle);

// SAFETY: the handle is only a pointer to the window system connection, which outlives the Gpu (see `run`) and is
// never written through it.
unsafe impl Send for Display {}
// SAFETY: as for `Send`.
unsafe impl Sync for Display {}

impl wgpu::rwh::HasDisplayHandle for Display {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        // SAFETY: the connection outlives the Gpu, and so this borrow of it.
        Ok(unsafe { wgpu::rwh::DisplayHandle::borrow_raw(self.0) })
    }
}

pub(super) fn best(
    adapters: impl IntoIterator<Item = wgpu::Adapter>,
    allow_cpu: bool,
) -> Result<wgpu::Adapter, String> {
    adapters
        .into_iter()
        .filter(|a| allow_cpu || a.get_info().device_type != wgpu::DeviceType::Cpu)
        .min_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::Cpu => 3,
            _ => 2,
        })
        .ok_or("no usable GPU adapter".to_string())
}
