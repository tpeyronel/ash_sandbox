use std::{
        borrow::Cow,
        cell::Cell,
        ffi::{c_void, CStr, CString},
        io::{self, Write},
        os::raw::c_char,
};

extern crate vk_mem as vma;

use std::rc::Rc;

use ash::{prelude::VkResult, vk};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use raw_window_handle::HasDisplayHandle;
use winit::window::Window;

use super::{
        vk_descriptor_set_allocator::VkDescriptorSetAllocator,
        vk_descriptor_set_layout_cache::VkDescriptorSetLayoutCache,
        vk_wrapper::{
                impl_destroyable_drop, VkCommandPool, VkDebugUtils, VkDevice, VkInstance, VkSurface, VmaAllocator,
        },
};
use crate::{
        vk::vk_wrapper::{VkPhysicalDevice, VkQueueFamilyIndices, VkQueues},
        AnyResult,
};

macro_rules! cstring {
        ($s:expr) => {
                CString::new($s).unwrap()
        };
}

pub struct VkContext {
        pub instance: Rc<VkInstance>,
        pub surface: Rc<VkSurface>,

        pub pdevice: Rc<VkPhysicalDevice>,
        pub device: Rc<VkDevice>,
        pub debug_utils: Option<Rc<VkDebugUtils>>,

        _qfamilyi: VkQueueFamilyIndices,
        pub queues: VkQueues,

        pub allocator: Rc<VmaAllocator>,
        pub cmd_pool: Rc<VkCommandPool>,
        pub dst_set_layout_cache: VkDescriptorSetLayoutCache,
        pub dst_set_allocator: VkDescriptorSetAllocator,

        destroyed: Cell<bool>,
}

#[cfg(all(debug_assertions))]
pub const ENABLE_VALIDATION_LAYERS: bool = true;
#[cfg(not(debug_assertions))]
pub const ENABLE_VALIDATION_LAYERS: bool = false;

impl VkContext {
        pub fn new(window: Rc<Window>) -> AnyResult<Self> {
                scoped_timer!("Initialized VkContext in: ", Millis);

                let entry = Rc::new(unsafe { ash::Entry::load()? });

                let vulkan_api_version = vk::make_api_version(0, 1, 3, 0);
                let instance = Self::create_instance(&window, &entry, vulkan_api_version)?;
                trace!("Created VkInstance");

                let surface = Rc::new(unsafe {
                        VkSurface::new(Rc::clone(&window), Rc::clone(&entry), Rc::clone(&instance))?
                });
                trace!("Created VkSurface");

                let (pdevice, qfamilyi) = VkPhysicalDevice::new(&instance, &surface)?;
                let pdevice = Rc::new(pdevice);

                trace!("Chose VkPhysicalDevice");
                info!("Chosen physical device: {:?}", unsafe {
                        CStr::from_ptr(pdevice.props.device_name.as_ptr())
                });
                info!("Queue family indices: {:?}", &qfamilyi);

                let (device, queues) = Self::create_device(&instance, **pdevice, &qfamilyi)?;
                trace!("Created VkDevice");

                let debug_utils = if !ENABLE_VALIDATION_LAYERS {
                        None
                } else {
                        let debug_cinfo = Self::create_debug_utils_messenger_cinfo();
                        let debug_utils = unsafe { VkDebugUtils::new(&entry, &instance, &device, &debug_cinfo)? };
                        trace!("Created VkDebugUtilsMessenger");
                        Some(Rc::new(debug_utils))
                };

                let allocator = Self::create_allocator(&instance, **pdevice, &device)?;
                trace!("Created VmaAllocator");

                let cmd_pool = Self::create_command_pool(&device, &qfamilyi)?;
                trace!("Created VkCommandPool");

                let dst_set_layout_cache = VkDescriptorSetLayoutCache::new(Rc::clone(&device));

                let dst_set_allocator = VkDescriptorSetAllocator::new(Rc::clone(&device))?;
                trace!("Created VkDescriptorPool");

                Ok(Self {
                        instance,
                        surface,

                        pdevice,
                        device,
                        debug_utils,

                        _qfamilyi: qfamilyi,
                        queues,

                        allocator,

                        cmd_pool,

                        dst_set_layout_cache,
                        dst_set_allocator,

                        destroyed: Cell::new(false),
                })
        }

        pub unsafe fn destroy(&mut self) {
                self.destroyed.set(true);

                let _ = self.device.device_wait_idle();
                self.dst_set_allocator.destroy();
                self.dst_set_layout_cache.destroy();
                self.cmd_pool.destroy();
                self.allocator.destroy();
                self.device.destroy();
                self.surface.destroy();
                if let Some(dum) = &self.debug_utils {
                        dum.destroy();
                }
                self.instance.destroy();
        }

        fn create_instance(
                window: &Window,
                entry: &Rc<ash::Entry>,
                vulkan_api_version: u32,
        ) -> AnyResult<Rc<VkInstance>> {
                unsafe {
                        let mut req_layers = Vec::new();
                        if ENABLE_VALIDATION_LAYERS {
                                req_layers.push(cstring!("VK_LAYER_KHRONOS_validation"));
                        }

                        let mut req_extensions: Vec<CString> =
                                ash_window::enumerate_required_extensions(window.display_handle()?.as_raw())?
                                        .iter()
                                        .map(|&ext| CStr::from_ptr(ext).to_owned())
                                        .collect();
                        req_extensions.push(cstring!("VK_EXT_debug_utils"));

                        let req_layers_raw: Vec<*const c_char> =
                                req_layers.iter().map(|layer| layer.as_ptr()).collect();

                        let req_extensions_raw: Vec<*const c_char> =
                                req_extensions.iter().map(|ext| ext.as_ptr()).collect();

                        info!("Required layers: {:?}", req_layers);
                        info!("Required extensions: {:?}", req_extensions);

                        let app_name = cstring!("ash_sandbox");

                        let app_info = vk::ApplicationInfo::default()
                                .application_name(&app_name)
                                .application_version(vk::make_api_version(0, 1, 0, 0))
                                .engine_name(&app_name)
                                .engine_version(vk::make_api_version(0, 1, 0, 0))
                                .api_version(vulkan_api_version);

                        let mut instance_cinfo = vk::InstanceCreateInfo::default()
                                .application_info(&app_info)
                                .enabled_layer_names(&req_layers_raw)
                                .enabled_extension_names(&req_extensions_raw);

                        let debug_info = Self::create_debug_utils_messenger_cinfo();

                        if ENABLE_VALIDATION_LAYERS {
                                instance_cinfo.p_next =
                                        &debug_info as *const vk::DebugUtilsMessengerCreateInfoEXT as *const c_void;
                        }

                        Ok(Rc::new(VkInstance::new(entry, &instance_cinfo)?))
                }
        }

        fn create_debug_utils_messenger_cinfo() -> vk::DebugUtilsMessengerCreateInfoEXT<'static> {
                vk::DebugUtilsMessengerCreateInfoEXT::default()
                        .message_severity(
                                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                        )
                        .message_type(
                                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                        )
                        .pfn_user_callback(Some(vk_debug_callback))
        }

        fn create_device(
                instance: &Rc<VkInstance>,
                physical_device: vk::PhysicalDevice,
                q_family_i: &VkQueueFamilyIndices,
        ) -> AnyResult<(Rc<VkDevice>, VkQueues)> {
                let req_device_extensions_raw = vec![
                        ash::khr::swapchain::NAME.as_ptr(),
                        ash::ext::memory_budget::NAME.as_ptr(),
                ];

                let req_device_features = vk::PhysicalDeviceFeatures::default()
                        .sampler_anisotropy(true)
                        .shader_clip_distance(true);

                let mut features13 = vk::PhysicalDeviceVulkan13Features::default()
                        .synchronization2(true)
                        .dynamic_rendering(true);
                let mut features = vk::PhysicalDeviceFeatures2::default()
                        .features(req_device_features)
                        .push_next(&mut features13);

                let queue_priorities;

                let device_q_cinfos = if q_family_i.graphics == q_family_i.present {
                        queue_priorities = vec![1.0];

                        vec![vk::DeviceQueueCreateInfo::default()
                                .queue_family_index(q_family_i.graphics)
                                .queue_priorities(&queue_priorities)]
                } else {
                        queue_priorities = vec![0.75, 0.25];

                        vec![
                                vk::DeviceQueueCreateInfo::default()
                                        .queue_family_index(q_family_i.graphics)
                                        .queue_priorities(&queue_priorities[0..1]),
                                vk::DeviceQueueCreateInfo::default()
                                        .queue_family_index(q_family_i.present)
                                        .queue_priorities(&queue_priorities[1..2]),
                        ]
                };

                let device_cinfo = vk::DeviceCreateInfo::default()
                        .queue_create_infos(&device_q_cinfos)
                        .enabled_extension_names(&req_device_extensions_raw)
                        // .enabled_features(&req_device_features)
                        .push_next(&mut features);

                let device = unsafe { Rc::new(VkDevice::new(instance, physical_device, &device_cinfo)?) };

                let queues = VkQueues {
                        graphics: unsafe { device.get_device_queue(q_family_i.graphics, 0) },
                        present: unsafe { device.get_device_queue(q_family_i.present, 0) },
                };

                Ok((device, queues))
        }

        fn create_allocator(
                instance: &ash::Instance,
                physical_device: vk::PhysicalDevice,
                device: &ash::Device,
        ) -> VkResult<Rc<VmaAllocator>> {
                Ok(Rc::new(unsafe {
                        VmaAllocator::new(instance, device, physical_device)?
                }))
        }

        fn create_command_pool(
                device: &Rc<VkDevice>,
                q_family_i: &VkQueueFamilyIndices,
        ) -> VkResult<Rc<VkCommandPool>> {
                let cmd_pool_cinfo = vk::CommandPoolCreateInfo::default()
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                        .queue_family_index(q_family_i.graphics);

                Ok(Rc::new(unsafe { VkCommandPool::new(device, &cmd_pool_cinfo)? }))
        }
}

impl_destroyable_drop!(VkContext);

unsafe extern "system" fn vk_debug_callback(
        message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
        message_type: vk::DebugUtilsMessageTypeFlagsEXT,
        p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
        _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
        if message_type == vk::DebugUtilsMessageTypeFlagsEXT::GENERAL {
                return vk::FALSE;
        }

        let callback_data = *p_callback_data;
        let message_id_number: i32 = callback_data.message_id_number as i32;

        let message_id_name = if callback_data.p_message_id_name.is_null() {
                Cow::from("")
        } else {
                CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
        };

        let message = if callback_data.p_message.is_null() {
                Cow::from("")
        } else {
                CStr::from_ptr(callback_data.p_message).to_string_lossy()
        };

        let display = format!(
                "Vulkan {:?}:\n{:?} [{} ({})] : {}",
                message_severity,
                message_type,
                message_id_name,
                &message_id_number.to_string(),
                message,
        );

        let mut stdout = io::stdout();
        let _ = stdout.flush();
        println!();
        match message_severity {
                vk::DebugUtilsMessageSeverityFlagsEXT::INFO => log::info!("{}", display),
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => log::warn!("{}", display),
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => log::error!("{}", display),
                vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => (),
                _ => log::info!("{}", display),
        }
        let _ = stdout.flush();

        if message_severity == vk::DebugUtilsMessageSeverityFlagsEXT::ERROR {
                std::hint::black_box(())
        }

        vk::FALSE
}
