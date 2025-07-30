use std::{cell::Cell, ops::Deref, rc::Rc};

use ash::{ext::debug_utils, khr::surface, prelude::VkResult, vk};
#[allow(unused_imports)]
use log::trace;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use thiserror::Error;

use crate::{vk::vk_context::VkContext, AnyResult};

use super::{vk_buffer::VkBuffer, vk_image::VkImage};

pub trait HasVkHandle<T: vk::Handle> {
        fn handle(self) -> T;
}

impl<T: vk::Handle> HasVkHandle<T> for T {
        fn handle(self) -> T {
                self
        }
}

macro_rules! impl_has_vk_handle {
        ($t:ty, $h:ty) => {
                impl HasVkHandle<$h> for &$t {
                        fn handle(self) -> $h {
                                self.handle
                        }
                }
        };
}

macro_rules! impl_destroyable_deref {
        ($t:ty, $h:ty) => {
                impl Deref for $t {
                        type Target = $h;

                        fn deref(&self) -> &Self::Target {
                                assert!(
                                        !self.destroyed.get(),
                                        "Tried to deref destroyed {}!",
                                        stringify!($t)
                                );

                                &self.handle
                        }
                }
        };
}

macro_rules! impl_destroyable_drop {
        ($t:ty) => {
                impl Drop for $t {
                        fn drop(&mut self) {
                                // assert!(
                                //         self.destroyed.get(),
                                //         "{} dropped but not destroyed!",
                                //         stringify!($t)
                                // );
                        }
                }
        };
}

macro_rules! impl_destroyable {
        ($t:ty, $h:ty, $d:ident $(, $args:expr)*) => {
                impl $t {
                        pub unsafe fn destroy(&self) {
                                assert!(!self.destroyed.get(), "Tried to destroy {} that has already been destroyed!", stringify!($t));

                                self.$d($($args),*);
                                self.destroyed.set(true);
                        }
                }

                impl_destroyable_deref!($t, $h);
                impl_destroyable_drop!($t);
        }
}

macro_rules! impl_destroyable_expr {
        ($t:ty, $h:ty, $c:expr) => {
                impl $t {
                        pub unsafe fn destroy(&self) {
                                assert!(
                                        !self.destroyed.get(),
                                        "Tried to destroy {} that has already been destroyed!",
                                        stringify!($t)
                                );

                                $c(self);
                                self.destroyed.set(true);
                        }
                }

                impl_destroyable_deref!($t, $h);
                impl_destroyable_drop!($t);
        };
}

pub(crate) use impl_destroyable;
pub(crate) use impl_destroyable_deref;
pub(crate) use impl_destroyable_drop;
pub(crate) use impl_destroyable_expr;

pub struct VkInstance {
        _entry: Rc<ash::Entry>,

        handle: ash::Instance,
        destroyed: Cell<bool>,
}

impl VkInstance {
        pub unsafe fn new(entry: &Rc<ash::Entry>, create_info: &vk::InstanceCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        _entry: Rc::clone(entry),

                        handle: entry.create_instance(create_info, None)?,
                        destroyed: Cell::new(false),
                })
        }
}

impl_destroyable!(VkInstance, ash::Instance, destroy_instance, None);

pub struct VmaAllocator {
        handle: vma::Allocator,
        destroyed: Cell<bool>,
}

impl VmaAllocator {
        pub unsafe fn new(
                instance: &ash::Instance,
                device: &ash::Device,
                physical_device: ash::vk::PhysicalDevice,
        ) -> VkResult<Self> {
                let create_info = vma::AllocatorCreateInfo::new(instance, device, physical_device);

                Ok(Self {
                        handle: vma::Allocator::new(create_info)?,
                        destroyed: Cell::new(false),
                })
        }
}

impl_destroyable_expr!(VmaAllocator, vma::Allocator, |s: &VmaAllocator| s.handle.destroy());

pub struct VkPhysicalDevice {
        handle: vk::PhysicalDevice,

        pub props: vk::PhysicalDeviceProperties,
        pub max_sampler_anisotropy: f32,
}

impl VkPhysicalDevice {
        pub fn new(instance: &ash::Instance, surface: &VkSurface) -> VkResult<(Self, VkQueueFamilyIndices)> {
                let (pdevice, qfamilies_indices) = Self::choose_physical_device(instance, surface)?;

                let props = unsafe { instance.get_physical_device_properties(pdevice) };

                Ok((
                        Self {
                                handle: pdevice,
                                props,
                                max_sampler_anisotropy: props.limits.max_sampler_anisotropy,
                        },
                        qfamilies_indices,
                ))
        }

        pub fn calc_padded_size(&self, size: usize) -> usize {
                let alignment = self.props.limits.min_uniform_buffer_offset_alignment as usize;

                if alignment > 0 {
                        (size + alignment - 1) & !(alignment - 1)
                } else {
                        size
                }
        }

        pub fn padded_size_of<T: 'static>(&self) -> usize {
                self.calc_padded_size(std::mem::size_of::<T>())
        }

        fn choose_physical_device(
                instance: &ash::Instance,
                surface: &VkSurface,
        ) -> VkResult<(vk::PhysicalDevice, VkQueueFamilyIndices)> {
                let pdevices = unsafe { instance.enumerate_physical_devices()? };

                Ok(pdevices
                        .iter()
                        .filter_map(|&pd| Self::is_device_suitable(instance, surface, pd))
                        .find(|&(pd, _)| unsafe {
                                let props = instance.get_physical_device_properties(pd);

                                props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
                        })
                        .expect("Couldn't find suitable VkPhysicalDevice"))
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
        pub present: u32,
}

impl VkQueueFamilyIndices {
        pub fn new(instance: &ash::Instance, surface: &VkSurface, pd: vk::PhysicalDevice) -> Option<Self> {
                let q_families_props = unsafe { instance.get_physical_device_queue_family_properties(pd) };

                fn find_queue_family<F>(q_families_props: &[vk::QueueFamilyProperties], cond: F) -> Option<u32>
                where
                        F: Fn(usize, &vk::QueueFamilyProperties) -> bool,
                {
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
                        surface.instance_loader()
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
                                present: graphics_and_present,
                        });
                }

                let graphics = find_queue_family(&q_families_props, supports_graphics);
                let present = find_queue_family(&q_families_props, supports_present);

                if let (Some(graphics), Some(present)) = (graphics, present) {
                        return Some(Self { graphics, present });
                }

                None
        }
}

pub struct VkQueues {
        pub graphics: vk::Queue,
        pub present: vk::Queue,
}

pub struct VkDevice {
        _instance: Rc<VkInstance>,

        handle: ash::Device,
        destroyed: Cell<bool>,
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
                        destroyed: Cell::new(false),
                })
        }
}

impl_destroyable!(VkDevice, ash::Device, destroy_device, None);

pub struct VkDebugUtils {
        _entry: Rc<ash::Entry>,

        instance_loader: debug_utils::Instance,
        device_loader: debug_utils::Device,
        handle: vk::DebugUtilsMessengerEXT,
        destroyed: Cell<bool>,
}

impl VkDebugUtils {
        pub unsafe fn new(
                entry: &Rc<ash::Entry>,
                instance: &ash::Instance,
                device: &ash::Device,
                create_info: &vk::DebugUtilsMessengerCreateInfoEXT,
        ) -> VkResult<Self> {
                let instance_loader = debug_utils::Instance::new(entry.deref(), instance);
                let device_loader = debug_utils::Device::new(instance, device);
                let handle = instance_loader.create_debug_utils_messenger(create_info, None)?;

                Ok(Self {
                        _entry: Rc::clone(entry),
                        instance_loader,
                        device_loader,
                        handle,
                        destroyed: Cell::new(false),
                })
        }

        #[allow(unused)]
        pub fn instance_loader(&self) -> &debug_utils::Instance {
                &self.instance_loader
        }

        #[allow(unused)]
        pub fn device_loader(&self) -> &debug_utils::Device {
                &self.device_loader
        }
}

impl_destroyable_expr!(VkDebugUtils, vk::DebugUtilsMessengerEXT, |s: &VkDebugUtils| s
        .instance_loader
        .destroy_debug_utils_messenger(s.handle, None));

pub struct VkSurface {
        _window: Rc<winit::window::Window>,
        _entry: Rc<ash::Entry>,
        _instance: Rc<VkInstance>,

        instance_loader: surface::Instance,
        handle: vk::SurfaceKHR,
        destroyed: Cell<bool>,
}

impl VkSurface {
        pub unsafe fn new(
                window: Rc<winit::window::Window>,
                entry: Rc<ash::Entry>,
                instance: Rc<VkInstance>,
        ) -> AnyResult<Self> {
                let instance_loader = surface::Instance::new(&entry, &instance);
                let handle = ash_window::create_surface(
                        &entry,
                        &instance,
                        window.display_handle()?.as_raw(),
                        window.window_handle()?.as_raw(),
                        None,
                )?;

                Ok(Self {
                        _window: window,
                        _entry: entry,
                        _instance: instance,

                        instance_loader,
                        handle,
                        destroyed: Cell::new(false),
                })
        }

        pub fn instance_loader(&self) -> &surface::Instance {
                &self.instance_loader
        }
}

impl_destroyable_expr!(VkSurface, vk::SurfaceKHR, |s: &VkSurface| s
        .instance_loader
        .destroy_surface(s.handle, None));

pub struct VkImageView {
        device: Rc<VkDevice>,

        handle: vk::ImageView,
        destroyed: Cell<bool>,
}

impl VkImageView {
        pub unsafe fn new(context: &VkContext, create_info: &vk::ImageViewCreateInfo) -> VkResult<Self> {
                let handle = context.device.create_image_view(create_info, None)?;

                Ok(Self {
                        device: Rc::clone(&context.device),
                        handle,
                        destroyed: Cell::new(false),
                })
        }
}

impl_has_vk_handle!(VkImageView, vk::ImageView);
impl_destroyable_expr!(VkImageView, vk::ImageView, |s: &VkImageView| s
        .device
        .destroy_image_view(s.handle, None));

pub struct VkSampler {
        device: Rc<VkDevice>,
        handle: vk::Sampler,
        destroyed: Cell<bool>,
}

impl VkSampler {
        pub unsafe fn new(context: &VkContext, create_info: &vk::SamplerCreateInfo) -> VkResult<Self> {
                let handle = context.device.create_sampler(create_info, None)?;

                Ok(Self {
                        device: Rc::clone(&context.device),
                        handle,
                        destroyed: Cell::new(false),
                })
        }
}

impl_has_vk_handle!(VkSampler, vk::Sampler);
impl_destroyable_expr!(VkSampler, vk::Sampler, |s: &VkSampler| s
        .device
        .destroy_sampler(s.handle, None));

pub struct VkCommandPool {
        device: Rc<VkDevice>,

        handle: vk::CommandPool,
        destroyed: Cell<bool>,
}

impl VkCommandPool {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::CommandPoolCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),

                        handle: device.create_command_pool(create_info, None)?,
                        destroyed: Cell::new(false),
                })
        }
}

impl_destroyable_expr!(VkCommandPool, vk::CommandPool, |s: &VkCommandPool| s
        .device
        .destroy_command_pool(s.handle, None));

pub struct VkPipelineLayout {
        device: Rc<VkDevice>,
        handle: vk::PipelineLayout,
        destroyed: Cell<bool>,
}

impl VkPipelineLayout {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::PipelineLayoutCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
                        handle: device.create_pipeline_layout(create_info, None)?,
                        destroyed: Cell::new(false),
                })
        }
}

impl_has_vk_handle!(VkPipelineLayout, vk::PipelineLayout);
impl_destroyable_expr!(VkPipelineLayout, vk::PipelineLayout, |s: &VkPipelineLayout| s
        .device
        .destroy_pipeline_layout(s.handle, None));

pub struct VkPipeline {
        device: Rc<VkDevice>,
        handle: vk::Pipeline,
        destroyed: Cell<bool>,
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
                                .map_err(|(_, vk_result)| vk_result)?[0],
                        destroyed: Cell::new(false),
                })
        }
}

impl_has_vk_handle!(VkPipeline, vk::Pipeline);
impl_destroyable_expr!(VkPipeline, vk::Pipeline, |s: &VkPipeline| s
        .device
        .destroy_pipeline(s.handle, None));

pub struct VkSemaphore {
        device: Rc<VkDevice>,
        handle: vk::Semaphore,
        destroyed: Cell<bool>,
}

impl VkSemaphore {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::SemaphoreCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
                        handle: device.create_semaphore(create_info, None)?,
                        destroyed: Cell::new(false),
                })
        }
}

impl_has_vk_handle!(VkSemaphore, vk::Semaphore);
impl_destroyable_expr!(VkSemaphore, vk::Semaphore, |s: &VkSemaphore| s
        .device
        .destroy_semaphore(s.handle, None));

pub struct VkFence {
        device: Rc<VkDevice>,
        handle: vk::Fence,
        destroyed: Cell<bool>,
}

impl VkFence {
        pub unsafe fn new(device: Rc<VkDevice>, create_info: &vk::FenceCreateInfo) -> VkResult<Self> {
                let handle = device.create_fence(create_info, None)?;

                Ok(Self {
                        device,
                        handle,
                        destroyed: Cell::new(false),
                })
        }
}

impl_has_vk_handle!(VkFence, vk::Fence);
impl_destroyable_expr!(VkFence, vk::Fence, |s: &VkFence| s.device.destroy_fence(s.handle, None));

#[derive(Error, Debug, Clone)]
pub enum VkShaderModuleError {
        #[error(transparent)]
        VkResult(#[from] vk::Result),
        #[error("shader code size is not a multiple of 4: {0}")]
        CodeSizeNotMultipleOf4(usize),
}

pub struct VkShaderModule {
        device: Rc<VkDevice>,
        handle: vk::ShaderModule,
        destroyed: Cell<bool>,
}

impl VkShaderModule {
        pub unsafe fn new(device: &Rc<VkDevice>, create_info: &vk::ShaderModuleCreateInfo) -> VkResult<Self> {
                Ok(Self {
                        device: Rc::clone(device),
                        handle: device.create_shader_module(create_info, None)?,
                        destroyed: Cell::new(false),
                })
        }

        pub fn from_code(device: &Rc<VkDevice>, code: &[u8]) -> Result<Self, VkShaderModuleError> {
                if code.len() % 4 != 0 {
                        return Err(VkShaderModuleError::CodeSizeNotMultipleOf4(code.len()));
                }

                let mut shader_module_cinfo = vk::ShaderModuleCreateInfo::default();
                shader_module_cinfo.code_size = code.len();
                shader_module_cinfo.p_code = code.as_ptr() as *const u32;

                Ok(unsafe { Self::new(device, &shader_module_cinfo)? })
        }
}

impl_has_vk_handle!(VkShaderModule, vk::ShaderModule);
impl_destroyable_expr!(VkShaderModule, vk::ShaderModule, |s: &VkShaderModule| s
        .device
        .destroy_shader_module(s.handle, None));

pub enum VkObject {
        Buffer(VkBuffer),
        Image(VkImage),
        ImageView(VkImageView),
        Sampler(VkSampler),
}

impl VkObject {
        pub unsafe fn destroy(&self) {
                match self {
                        VkObject::Buffer(b) => b.destroy(),
                        VkObject::Image(i) => i.destroy(),
                        VkObject::ImageView(iv) => iv.destroy(),
                        VkObject::Sampler(s) => s.destroy(),
                }
        }
}
