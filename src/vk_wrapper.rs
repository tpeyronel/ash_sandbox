use ash::extensions::ext::DebugUtils;
use ash::prelude::VkResult;
use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0};
use ash::vk;
use std::ops::Deref;
use std::sync::Arc;

pub struct VkInstance {
        entry: Arc<ash::Entry>,
        handle: ash::Instance,
}

impl VkInstance {
        pub unsafe fn new(
                entry: &Arc<ash::Entry>,
                create_info: &vk::InstanceCreateInfo,
        ) -> Result<Self, ash::InstanceError> {
                Ok(Self {
                        entry: Arc::clone(entry),
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
                        //self.destroy_instance(None);
                }
        }
}

pub struct VkDevice {
        handle: ash::Device,
}

impl VkDevice {
        pub unsafe fn new(
                instance: &ash::Instance,
                physical_device: vk::PhysicalDevice,
                create_info: &vk::DeviceCreateInfo,
        ) -> VkResult<Self> {
                Ok(Self {
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
                        self.destroy_device(None);
                }
        }
}

pub struct VkDebugUtilsMessenger {
        loader: DebugUtils,
        handle: vk::DebugUtilsMessengerEXT,
}

impl VkDebugUtilsMessenger {
        pub unsafe fn new(
                entry: &ash::Entry,
                instance: &ash::Instance,
                create_info: &vk::DebugUtilsMessengerCreateInfoEXT,
        ) -> VkResult<Self> {
                let loader = DebugUtils::new(entry, instance);
                let handle = loader.create_debug_utils_messenger(create_info, None)?;

                Ok(Self { loader, handle })
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
        loader: ash::extensions::khr::Surface,
        handle: vk::SurfaceKHR,
}

impl VkSurface {
        pub unsafe fn new(
                entry: &ash::Entry,
                instance: &ash::Instance,
                window: &winit::window::Window,
        ) -> VkResult<Self> {
                let loader = ash::extensions::khr::Surface::new(entry, instance);
                let handle = ash_window::create_surface(entry, instance, window, None)?;

                Ok(Self { loader, handle })
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
                        self.device.destroy_framebuffer(self.handle, None);
                }
        }
}
