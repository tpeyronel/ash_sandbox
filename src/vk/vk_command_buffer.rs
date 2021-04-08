use std::{error::Error, ops::Deref, rc::Rc};

use ash::{prelude::VkResult, version::DeviceV1_0, vk};

use super::vk_wrapper::{VkCommandPool, VkDevice, VkFence};

pub struct VkReusableCommandBuffer {
        device:   Rc<VkDevice>,
        cmd_pool: Rc<VkCommandPool>,

        handle:    vk::CommandBuffer,
        pub fence: VkFence,
}

impl VkReusableCommandBuffer {
        pub fn new(device: &Rc<VkDevice>, cmd_pool: &Rc<VkCommandPool>) -> VkResult<Self> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(***cmd_pool)
                        .command_buffer_count(1)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handle = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)?[0] };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
                let fence = unsafe { VkFence::new(device, &fence_cinfo)? };

                Ok(Self {
                        device: Rc::clone(device),
                        cmd_pool: Rc::clone(cmd_pool),

                        handle,
                        fence,
                })
        }

        pub fn new_vec(device: &Rc<VkDevice>, cmd_pool: &Rc<VkCommandPool>, count: u32) -> VkResult<Vec<Self>> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(***cmd_pool)
                        .command_buffer_count(count)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handles = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)? };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

                handles.iter()
                        .map(|&handle| {
                                let fence = unsafe { VkFence::new(device, &fence_cinfo)? };

                                Ok(Self {
                                        device: Rc::clone(device),
                                        cmd_pool: Rc::clone(cmd_pool),

                                        handle,
                                        fence,
                                })
                        })
                        .collect()
        }

        pub fn record_and_submit<F>(
                &self,
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
                        self.device.wait_for_fences(&[*self.fence], true, u64::MAX)?;

                        self.device.reset_fences(&[*self.fence])?;
                        self.device
                                .reset_command_buffer(self.handle, vk::CommandBufferResetFlags::RELEASE_RESOURCES)?;

                        let cmd_buffer_binfo = vk::CommandBufferBeginInfo::builder()
                                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                        self.device.begin_command_buffer(self.handle, &cmd_buffer_binfo)?;
                        f(&self.device, self.handle)?;
                        self.device.end_command_buffer(self.handle)?;

                        let cmd_buffers = [self.handle];

                        let submit_info = vk::SubmitInfo::builder()
                                .command_buffers(&cmd_buffers)
                                .wait_semaphores(wait_semaphores)
                                .wait_dst_stage_mask(wait_stages)
                                .signal_semaphores(signal_semaphores);

                        self.device
                                .queue_submit(submit_queue, &[submit_info.build()], *self.fence)?;

                        Ok(())
                }
        }

        pub unsafe fn begin(&self, device: &ash::Device) -> VkResult<()> {
                device.wait_for_fences(&[*self.fence], true, u64::MAX)?;
                device.reset_fences(&[*self.fence])?;
                device.reset_command_buffer(self.handle, vk::CommandBufferResetFlags::RELEASE_RESOURCES)?;

                let cmd_buffer_binfo =
                        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                device.begin_command_buffer(self.handle, &cmd_buffer_binfo)
        }

        pub unsafe fn end_and_submit(
                &self,
                device: &ash::Device,
                submit_queue: vk::Queue,
                wait_semaphores: &[vk::Semaphore],
                wait_stages: &[vk::PipelineStageFlags],
                signal_semaphores: &[vk::Semaphore],
        ) -> VkResult<()> {
                device.end_command_buffer(self.handle)?;

                let submit_info = vk::SubmitInfo::builder()
                        .command_buffers(std::slice::from_ref(&self.handle))
                        .wait_semaphores(wait_semaphores)
                        .wait_dst_stage_mask(wait_stages)
                        .signal_semaphores(signal_semaphores)
                        .build();

                device.queue_submit(submit_queue, std::slice::from_ref(&submit_info), *self.fence)
        }

        pub unsafe fn wait(&self, device: &ash::Device, timeout: u64) -> VkResult<()> {
                device.wait_for_fences(&[*self.fence], true, timeout)
        }
}

impl Deref for VkReusableCommandBuffer {
        type Target = vk::CommandBuffer;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkReusableCommandBuffer {
        fn drop(&mut self) {
                unsafe {
                        self.device
                                .free_command_buffers(**self.cmd_pool, std::slice::from_ref(&self.handle));
                }
        }
}
