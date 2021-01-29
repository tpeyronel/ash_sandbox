use std::{ops::Deref, sync::Arc};

use ash::{
        extensions::ext::DebugUtils,
        prelude::VkResult,
        version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
        vk,
};
use log::trace;








pub struct VkInstance {
        _entry: Arc<ash::Entry>,

        handle: ash::Instance,
}

impl VkInstance {
        pub unsafe fn new(
                entry: &Arc<ash::Entry>,
                create_info: &vk::InstanceCreateInfo,
        ) -> Result<Self, ash::InstanceError> {
                Ok(Self {
                        _entry: Arc::clone(entry),

                        handle: entry.create_instance(create_info, None)?,
                })
        }
}

impl Deref for VkInstance {
        type Target = ash::Instance;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkInstance {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkInstance...");

                        self.destroy_instance(None);
                }
        }
}








pub struct VkDevice {
        _instance: Arc<VkInstance>,

        handle: ash::Device,
}

impl VkDevice {
        pub unsafe fn new(
                instance: &Arc<VkInstance>,
                physical_device: vk::PhysicalDevice,
                create_info: &vk::DeviceCreateInfo,
        ) -> VkResult<Self> {
                Ok(Self {
                        _instance: Arc::clone(instance),

                        handle: instance.create_device(physical_device, create_info, None)?,
                })
        }
}

impl Deref for VkDevice {
        type Target = ash::Device;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkDevice {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkDevice...");

                        self.destroy_device(None);
                }
        }
}








pub struct VkDebugUtilsMessenger {
        _entry: Arc<ash::Entry>,

        loader: DebugUtils,
        handle: vk::DebugUtilsMessengerEXT,
}

impl VkDebugUtilsMessenger {
        pub unsafe fn new(
                entry: &Arc<ash::Entry>,
                instance: &ash::Instance,
                create_info: &vk::DebugUtilsMessengerCreateInfoEXT,
        ) -> VkResult<Self> {
                let loader = DebugUtils::new(entry.deref(), instance);
                let handle = loader.create_debug_utils_messenger(create_info, None)?;

                Ok(Self {
                        _entry: Arc::clone(entry),
                        loader,
                        handle,
                })
        }
}

impl Deref for VkDebugUtilsMessenger {
        type Target = vk::DebugUtilsMessengerEXT;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkDebugUtilsMessenger {
        fn drop(&mut self) {
                unsafe {
                        self.loader.destroy_debug_utils_messenger(self.handle, None);
                }
        }
}








pub struct VkSurface {
        _window:   Arc<winit::window::Window>,
        _entry:    Arc<ash::Entry>,
        _instance: Arc<VkInstance>,

        loader: ash::extensions::khr::Surface,
        handle: vk::SurfaceKHR,
}

impl VkSurface {
        pub unsafe fn new(
                window: &Arc<winit::window::Window>,
                entry: &Arc<ash::Entry>,
                instance: &Arc<VkInstance>,
        ) -> VkResult<Self> {
                let loader = ash::extensions::khr::Surface::new(entry.deref(), &***instance);
                let handle = ash_window::create_surface(entry.deref(), &***instance, &**window, None)?;

                Ok(Self {
                        _window: Arc::clone(window),
                        _entry: Arc::clone(entry),
                        _instance: Arc::clone(instance),

                        loader,
                        handle,
                })
        }

        pub fn loader(&self) -> &ash::extensions::khr::Surface {
                &self.loader
        }
}

impl Deref for VkSurface {
        type Target = vk::SurfaceKHR;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkSurface {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkSurface...");

                        self.loader.destroy_surface(self.handle, None);
                }
        }
}








pub struct VkImageView {
        device: Arc<VkDevice>,
        handle: vk::ImageView,
}

impl VkImageView {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::ImageViewCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_image_view(create_info, None)?,
                })
        }
}

impl Deref for VkImageView {
        type Target = vk::ImageView;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkImageView {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkImageView...");

                        self.device.destroy_image_view(self.handle, None);
                }
        }
}








pub struct VkSampler {
        device: Arc<VkDevice>,
        handle: vk::Sampler,
}

impl VkSampler {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::SamplerCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_sampler(create_info, None)?,
                })
        }
}

impl Deref for VkSampler {
        type Target = vk::Sampler;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkSampler {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkSampler...");

                        self.device.destroy_sampler(self.handle, None);
                }
        }
}








pub struct VkFramebuffer {
        device: Arc<VkDevice>,
        handle: vk::Framebuffer,
}

impl VkFramebuffer {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::FramebufferCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_framebuffer(create_info, None)?,
                })
        }
}

impl Deref for VkFramebuffer {
        type Target = vk::Framebuffer;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkFramebuffer {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkFramebuffer...");

                        self.device.destroy_framebuffer(self.handle, None);
                }
        }
}








pub struct VkRenderPass {
        device: Arc<VkDevice>,
        handle: vk::RenderPass,
}

impl VkRenderPass {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::RenderPassCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_render_pass(create_info, None)?,
                })
        }
}

impl Deref for VkRenderPass {
        type Target = vk::RenderPass;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkRenderPass {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkRenderPass...");

                        self.device.destroy_render_pass(self.handle, None);
                }
        }
}








pub struct VkCommandPool {
        device: Arc<VkDevice>,

        handle: vk::CommandPool,
}

impl VkCommandPool {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::CommandPoolCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),

                        handle: device.create_command_pool(create_info, None)?,
                })
        }
}

impl Deref for VkCommandPool {
        type Target = vk::CommandPool;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkCommandPool {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkCommandPool...");

                        self.device.destroy_command_pool(self.handle, None);
                }
        }
}








pub struct VkDescriptorPool {
        device: Arc<VkDevice>,

        handle: vk::DescriptorPool,
}

impl VkDescriptorPool {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::DescriptorPoolCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),

                        handle: device.create_descriptor_pool(create_info, None)?,
                })
        }
}

impl Deref for VkDescriptorPool {
        type Target = vk::DescriptorPool;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkDescriptorPool {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkDescriptorPool...");

                        self.device.destroy_descriptor_pool(self.handle, None);
                }
        }
}








pub struct VkDescriptorSetLayout {
        device: Arc<VkDevice>,
        handle: vk::DescriptorSetLayout,
}

impl VkDescriptorSetLayout {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::DescriptorSetLayoutCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_descriptor_set_layout(create_info, None)?,
                })
        }
}

impl Deref for VkDescriptorSetLayout {
        type Target = vk::DescriptorSetLayout;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkDescriptorSetLayout {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkDescriptorSetLayout...");

                        self.device.destroy_descriptor_set_layout(self.handle, None);
                }
        }
}








pub struct VkPipelineLayout {
        device: Arc<VkDevice>,
        handle: vk::PipelineLayout,
}

impl VkPipelineLayout {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::PipelineLayoutCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_pipeline_layout(create_info, None)?,
                })
        }
}

impl Deref for VkPipelineLayout {
        type Target = vk::PipelineLayout;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkPipelineLayout {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkPipelineLayout...");

                        self.device.destroy_pipeline_layout(self.handle, None);
                }
        }
}








pub struct VkPipeline {
        device: Arc<VkDevice>,
        handle: vk::Pipeline,
}

impl VkPipeline {
        pub unsafe fn new_graphics(
                device: &Arc<VkDevice>,
                pipeline_cache: vk::PipelineCache,
                create_info: &vk::GraphicsPipelineCreateInfo,
        ) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device
                                .create_graphics_pipelines(pipeline_cache, std::slice::from_ref(create_info), None)
                                .map_err(|(_, result)| result)?[0],
                })
        }
}

impl Deref for VkPipeline {
        type Target = vk::Pipeline;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkPipeline {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkPipeline...");

                        self.device.destroy_pipeline(self.handle, None);
                }
        }
}








pub struct VkSemaphore {
        device: Arc<VkDevice>,
        handle: vk::Semaphore,
}

impl VkSemaphore {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::SemaphoreCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_semaphore(create_info, None)?,
                })
        }
}

impl Deref for VkSemaphore {
        type Target = vk::Semaphore;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkSemaphore {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkSemaphore...");

                        self.device.destroy_semaphore(self.handle, None);
                }
        }
}








pub struct VkFence {
        device: Arc<VkDevice>,
        handle: vk::Fence,
}

impl VkFence {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::FenceCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_fence(create_info, None)?,
                })
        }
}

impl Deref for VkFence {
        type Target = vk::Fence;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkFence {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkFence...");

                        self.device.destroy_fence(self.handle, None);
                }
        }
}








pub struct VkShaderModule {
        device: Arc<VkDevice>,
        handle: vk::ShaderModule,
}

impl VkShaderModule {
        pub unsafe fn new(device: &Arc<VkDevice>, create_info: &vk::ShaderModuleCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Arc::clone(device),
                        handle: device.create_shader_module(create_info, None)?,
                })
        }
}

impl Deref for VkShaderModule {
        type Target = vk::ShaderModule;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkShaderModule {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkShaderModule...");

                        self.device.destroy_shader_module(self.handle, None);
                }
        }
}
