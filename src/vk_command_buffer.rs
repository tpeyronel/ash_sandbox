use std::{error::Error, ops::Deref, sync::Arc};

use ash::{prelude::VkResult, version::DeviceV1_0, vk};

use crate::vk_wrapper::{VkDevice, VkFence};

pub struct VkReusableCommandBuffer {
        handle:    vk::CommandBuffer,
        pub fence: VkFence,
}

impl VkReusableCommandBuffer {
        pub fn new(device: &Arc<VkDevice>, cmd_pool: vk::CommandPool) -> VkResult<Self> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(cmd_pool)
                        .command_buffer_count(1)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handle = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)?[0] };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
                let fence = unsafe { VkFence::new(device, &fence_cinfo)? };

                Ok(Self {
                        handle,
                        fence,
                })
        }

        pub fn new_vec(device: &Arc<VkDevice>, cmd_pool: vk::CommandPool, count: u32) -> VkResult<Vec<Self>> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(cmd_pool)
                        .command_buffer_count(count)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handles = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)? };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

                handles.iter()
                        .map(|&handle| {
                                let fence = unsafe { VkFence::new(device, &fence_cinfo)? };

                                Ok(Self {
                                        handle,
                                        fence,
                                })
                        })
                        .collect()
        }

        pub fn record_and_submit<F>(
                &self,
                device: &ash::Device,
                submit_queue: vk::Queue,
                wait_semaphores: &[vk::Semaphore],
                wait_stages: &[vk::PipelineStageFlags],
                signal_semaphores: &[vk::Semaphore],
                f: F,
        ) -> Result<(), Box<dyn Error>>
        where
                F: FnOnce(&ash::Device, vk::CommandBuffer) -> Result<(), Box<dyn Error>>,
        {
                unsafe {
                        {
                                //let t = Timer::new("wait_for_fences took: ");

                                device.wait_for_fences(&[*self.fence], true, u64::MAX)?;
                        }
                        device.reset_fences(&[*self.fence])?;
                        device.reset_command_buffer(self.handle, vk::CommandBufferResetFlags::RELEASE_RESOURCES)?;

                        let cmd_buffer_binfo = vk::CommandBufferBeginInfo::builder()
                                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                        device.begin_command_buffer(self.handle, &cmd_buffer_binfo)?;
                        f(&device, self.handle)?;
                        device.end_command_buffer(self.handle)?;

                        let cmd_buffers = [self.handle];

                        let submit_info = vk::SubmitInfo::builder()
                                .command_buffers(&cmd_buffers)
                                .wait_semaphores(wait_semaphores)
                                .wait_dst_stage_mask(wait_stages)
                                .signal_semaphores(signal_semaphores);

                        device.queue_submit(submit_queue, &[submit_info.build()], *self.fence)?;

                        Ok(())
                }
        }

        pub fn wait(&self, device: &ash::Device, timeout: u64) -> VkResult<()> {
                unsafe { device.wait_for_fences(&[*self.fence], true, timeout) }
        }
}

impl Deref for VkReusableCommandBuffer {
        type Target = vk::CommandBuffer;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}
