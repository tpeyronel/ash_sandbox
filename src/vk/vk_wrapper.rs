use std::{ffi::CStr, ops::Deref, rc::Rc};

use ash::{
        extensions::ext::DebugUtils,
        prelude::VkResult,
        version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
        vk,
};
use log::trace;


pub struct VkInstance {
        _entry: Rc<ash::Entry>,

        handle: ash::Instance,
}

impl VkInstance {
        pub unsafe fn new(
                entry: &Rc<ash::Entry>,
                create_info: &vk::InstanceCreateInfo,
        ) -> Result<Self, ash::InstanceError> {
                Ok(Self {
                        _entry: Rc::clone(entry),

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








pub struct VkPhysicalDevice {
        handle: vk::PhysicalDevice,

        pub props:                  vk::PhysicalDeviceProperties,
        pub max_sampler_anisotropy: f32,
}

impl VkPhysicalDevice {
        pub fn new(instance: &ash::Instance, surface: &VkSurface) -> VkResult<(Self, VkQueueFamilyIndices)> {
                let (pdevice, qfamilies_indices) = Self::choose_physical_device(instance, surface)?;

                let props = unsafe { instance.get_physical_device_properties(pdevice) };
                let limits = &props.limits;

                Ok((
                        Self {
                                handle: pdevice,
                                props,
                                max_sampler_anisotropy: limits.max_sampler_anisotropy,
                        },
                        qfamilies_indices,
                ))
        }

        fn choose_physical_device(
                instance: &ash::Instance,
                surface: &VkSurface,
        ) -> VkResult<(vk::PhysicalDevice, VkQueueFamilyIndices)> {
                let pdevices = unsafe { instance.enumerate_physical_devices()? };

                Ok(pdevices
                        .iter()
                        .filter_map(|&pd| Self::is_device_suitable(instance, surface, pd))
                        .find(|&(pd, _)| {
                                let name = unsafe {
                                        CStr::from_ptr(instance.get_physical_device_properties(pd).device_name.as_ptr())
                                                .to_str()
                                                .unwrap()
                                };

                                name == "NVIDIA GeForce GTX 970"
                        })
                        .unwrap())
        }

        fn is_device_suitable(
                instance: &ash::Instance,
                surface: &VkSurface,
                pd: vk::PhysicalDevice,
        ) -> Option<(vk::PhysicalDevice, VkQueueFamilyIndices)> {
                let q_family_i = match VkQueueFamilyIndices::new(instance, surface, pd) {
                        Some(q_family_i) => q_family_i,
                        None => return None,
                };

                let supported_features = unsafe { instance.get_physical_device_features(pd) };

                if supported_features.sampler_anisotropy == vk::FALSE {
                        return None;
                }

                Some((pd, q_family_i))
        }
}

impl Deref for VkPhysicalDevice {
        type Target = vk::PhysicalDevice;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}








#[derive(Debug)]
pub struct VkQueueFamilyIndices {
        pub graphics: u32,
        pub present:  u32,
}

impl VkQueueFamilyIndices {
        pub fn new(instance: &ash::Instance, surface: &VkSurface, pd: vk::PhysicalDevice) -> Option<Self> {
                let q_families_props = unsafe { instance.get_physical_device_queue_family_properties(pd) };

                fn find_queue_family<F>(q_families_props: &[vk::QueueFamilyProperties], cond: F) -> Option<u32>
                where F: Fn(usize, &vk::QueueFamilyProperties) -> bool {
                        q_families_props
                                .iter()
                                .enumerate()
                                .filter_map(
                                        |(i, q_fam_props)| {
                                                if cond(i, q_fam_props) {
                                                        Some(i as u32)
                                                } else {
                                                        None
                                                }
                                        },
                                )
                                .next()
                }

                let supports_graphics = |_i: usize, q_fam_props: &vk::QueueFamilyProperties| {
                        q_fam_props.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                };

                let supports_present = |i: usize, _q_fam_props: &vk::QueueFamilyProperties| unsafe {
                        surface.loader()
                                .get_physical_device_surface_support(pd, i as u32, **surface)
                                .unwrap()
                };

                let supports_both = |i: usize, q_fam_props: &vk::QueueFamilyProperties| {
                        supports_graphics(i, q_fam_props) && supports_present(i, q_fam_props)
                };

                let graphics_and_present = find_queue_family(&q_families_props, supports_both);

                if let Some(graphics_and_present) = graphics_and_present {
                        return Some(Self {
                                graphics: graphics_and_present,
                                present:  graphics_and_present,
                        });
                }

                let graphics = find_queue_family(&q_families_props, supports_graphics);
                let present = find_queue_family(&q_families_props, supports_present);

                if let (Some(graphics), Some(present)) = (graphics, present) {
                        return Some(Self {
                                graphics,
                                present,
                        });
                }

                None
        }
}








pub struct VkQueues {
        pub graphics: vk::Queue,
        pub present:  vk::Queue,
}








pub struct VkDevice {
        _instance: Rc<VkInstance>,

        handle: ash::Device,
}

impl VkDevice {
        pub unsafe fn new(
                instance: &Rc<VkInstance>,
                physical_device: vk::PhysicalDevice,
                create_info: &vk::DeviceCreateInfo,
        ) -> VkResult<Self> {
                Ok(Self {
                        _instance: Rc::clone(instance),

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
        _entry: Rc<ash::Entry>,

        loader: DebugUtils,
        handle: vk::DebugUtilsMessengerEXT,
}

impl VkDebugUtilsMessenger {
        pub unsafe fn new(
                entry: &Rc<ash::Entry>,
                instance: &ash::Instance,
                create_info: &vk::DebugUtilsMessengerCreateInfoEXT,
        ) -> VkResult<Self> {
                let loader = DebugUtils::new(entry.deref(), instance);
                let handle = loader.create_debug_utils_messenger(create_info, None)?;

                Ok(Self {
                        _entry: Rc::clone(entry),
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
        _window:   Rc<winit::window::Window>,
        _entry:    Rc<ash::Entry>,
        _instance: Rc<VkInstance>,

        loader: ash::extensions::khr::Surface,
        handle: vk::SurfaceKHR,
}

impl VkSurface {
        pub unsafe fn new(
                window: Rc<winit::window::Window>,
                entry: Rc<ash::Entry>,
                instance: Rc<VkInstance>,
        ) -> VkResult<Self> {
                let loader = ash::extensions::khr::Surface::new(entry.deref(), &**instance);
                let handle = ash_window::create_surface(entry.deref(), &**instance, &*window, None)?;

                Ok(Self {
                        _window: window,
                        _entry: entry,
                        _instance: instance,

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
        device: Rc<VkDevice>,

        handle: vk::ImageView,
}

impl VkImageView {
        pub unsafe fn new(device: Rc<VkDevice>, create_info: &vk::ImageViewCreateInfo) -> VkResult<Self> {
                let handle = device.create_image_view(create_info, None)?;

                Ok(Self {
                        device,
                        handle,
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
        device: Rc<VkDevice>,
        handle: vk::Sampler,
}

impl VkSampler {
        pub unsafe fn new(device: Rc<VkDevice>, create_info: &vk::SamplerCreateInfo) -> VkResult<Self> {
                let handle = device.create_sampler(create_info, None)?;

                Ok(Self {
                        device,
                        handle,
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
        device: Rc<VkDevice>,
        handle: vk::Framebuffer,
}

impl VkFramebuffer {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::FramebufferCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
        device: Rc<VkDevice>,
        handle: vk::RenderPass,
}

impl VkRenderPass {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::RenderPassCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
        device: Rc<VkDevice>,

        handle: vk::CommandPool,
}

impl VkCommandPool {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::CommandPoolCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),

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
        device: Rc<VkDevice>,

        handle: vk::DescriptorPool,
}

impl VkDescriptorPool {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::DescriptorPoolCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),

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
        device: Rc<VkDevice>,
        handle: vk::DescriptorSetLayout,
}

impl VkDescriptorSetLayout {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::DescriptorSetLayoutCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
        device: Rc<VkDevice>,
        handle: vk::PipelineLayout,
}

impl VkPipelineLayout {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::PipelineLayoutCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
        device: Rc<VkDevice>,
        handle: vk::Pipeline,
}

impl VkPipeline {
        pub unsafe fn new_graphics(
                device: &Rc<VkDevice>,
                pipeline_cache: vk::PipelineCache,
                create_info: &vk::GraphicsPipelineCreateInfo,
        ) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
        device: Rc<VkDevice>,
        handle: vk::Semaphore,
}

impl VkSemaphore {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::SemaphoreCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
        device: Rc<VkDevice>,
        handle: vk::Fence,
}

impl VkFence {
        pub unsafe fn new(device: Rc<VkDevice>, create_info: &vk::FenceCreateInfo) -> VkResult<Self> {
                let handle = device.create_fence(create_info, None)?;

                Ok(Self {
                        device,
                        handle,
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
        device: Rc<VkDevice>,
        handle: vk::ShaderModule,
}

impl VkShaderModule {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::ShaderModuleCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
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
