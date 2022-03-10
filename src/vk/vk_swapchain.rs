use std::{cell::Cell, ops::Deref, rc::Rc};

use ash::{extensions::khr::Swapchain, prelude::VkResult, vk};
use bitflags::bitflags;
#[allow(unused_imports)]
use log::{debug, trace};

use crate::AnyResult;

use super::{
        vk_image::{VkImage, VkImageCreateInfo},
        vk_wrapper::{
                impl_destroyable_deref, impl_destroyable_drop, impl_destroyable_expr, VkDevice, VkFramebuffer,
                VkImageView, VkInstance, VkSurface, VmaAllocator,
        },
};

pub struct VkSwapchain {
        loader: Swapchain,

        window: Rc<winit::window::Window>,
        instance: Rc<VkInstance>,
        surface: Rc<VkSurface>,
        physical_device: vk::PhysicalDevice,
        device: Rc<VkDevice>,
        allocator: Rc<VmaAllocator>,
        desired_img_count: u32,

        handle: vk::SwapchainKHR,
        destroyed: Cell<bool>,

        pub color_format: vk::SurfaceFormatKHR,
        pub depth_format: vk::Format,
        pub extent: vk::Extent2D,
        pub viewport: vk::Viewport,
        pub scissor: vk::Rect2D,
        pub present_mode: vk::PresentModeKHR,
        pub samples: vk::SampleCountFlags,

        pub color_img: VkImage,
        pub color_img_view: VkImageView,

        pub depth_img: VkImage,
        pub depth_img_view: VkImageView,

        pub resolve_imgs: Vec<vk::Image>,
        pub resolve_img_views: Vec<VkImageView>,
        pub img_count: u32,
        pub framebuffers: Vec<VkFramebuffer>,
}

impl VkSwapchain {
        pub fn new(
                window: Rc<winit::window::Window>,
                instance: Rc<VkInstance>,
                surface: Rc<VkSurface>,
                physical_device: vk::PhysicalDevice,
                device: Rc<VkDevice>,
                allocator: Rc<VmaAllocator>,
                desired_img_count: u32,
        ) -> AnyResult<Self> {
                let color_format = Self::choose_color_format(&surface, physical_device)?;
                debug!("VkSwapchain color format ({:?})", color_format);
                let depth_format = vk::Format::D24_UNORM_S8_UINT;

                let surface_capabilities = unsafe {
                        surface.loader()
                                .get_physical_device_surface_capabilities(physical_device, **surface)?
                };

                let requested_img_count = Self::clamp_image_count(desired_img_count, &surface_capabilities);
                debug!("VkSwapchain image count: {}", requested_img_count);

                let extent = Self::create_extent(&window, &surface_capabilities);
                debug!("VkSwapchain extent: {:?}", extent);

                let viewport = Self::create_viewport(&extent);
                let scissor = Self::create_scissor(&extent);

                let pre_transform = surface_capabilities.current_transform;

                let present_mode = Self::choose_present_mode(&surface, physical_device)?;
                debug!("VkSwapchain present mode: {:?}", present_mode);

                let loader = Swapchain::new(&**instance, &**device);

                let swch_cinfo = vk::SwapchainCreateInfoKHR::builder()
                        .surface(**surface)
                        .min_image_count(requested_img_count)
                        .image_color_space(color_format.color_space)
                        .image_format(color_format.format)
                        .image_extent(extent)
                        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .pre_transform(pre_transform)
                        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                        .present_mode(present_mode)
                        .clipped(true)
                        .image_array_layers(1)
                        .old_swapchain(vk::SwapchainKHR::null());

                let handle = unsafe { loader.create_swapchain(&swch_cinfo, None)? };

                let samples = Self::choose_sample_count(&instance, physical_device);
                debug!("Swapchain samples: {:?}", samples);

                let (color_img, color_img_view) = Self::create_color_img_resources(
                        Rc::clone(&device),
                        Rc::clone(&allocator),
                        color_format.format,
                        &extent,
                        samples,
                )?;
                let (depth_img, depth_img_view) = Self::create_depth_img_resources(
                        Rc::clone(&device),
                        Rc::clone(&allocator),
                        depth_format,
                        &extent,
                        samples,
                )?;

                let resolve_imgs = unsafe { loader.get_swapchain_images(handle)? };
                let resolve_img_views =
                        Self::create_resolve_img_views(Rc::clone(&device), &resolve_imgs, color_format.format)?;

                let img_count = resolve_imgs.len() as u32;

                Ok(Self {
                        loader,

                        window,
                        instance,
                        surface,
                        physical_device,
                        device,
                        allocator,
                        desired_img_count,

                        handle,
                        destroyed: Cell::new(false),

                        color_format,
                        depth_format,

                        extent,
                        viewport,
                        scissor,
                        present_mode,
                        samples,

                        color_img,
                        color_img_view,

                        depth_img,
                        depth_img_view,

                        resolve_imgs,
                        resolve_img_views,
                        img_count,

                        framebuffers: vec![],
                })
        }

        pub fn recreate(&mut self) -> VkResult<VkSwapchainRecreationInfo> {
                self.framebuffers.drain(..).for_each(|fb| unsafe { fb.destroy() });

                let mut recreation_info = VkSwapchainRecreationInfo {
                        color_format_changed: false,
                        extent_changed: false,
                        samples_changed: false,
                        img_count_changed: false,
                };

                let old_color_format = self.color_format;
                self.color_format = Self::choose_color_format(&self.surface, self.physical_device)?;
                recreation_info.color_format_changed = old_color_format != self.color_format;
                if recreation_info.color_format_changed {
                        debug!("VkSwapchain color format ({:?})", self.color_format);
                }

                let depth_format = vk::Format::D24_UNORM_S8_UINT;
                self.depth_format = depth_format;

                let surface_capabilities = unsafe {
                        self.surface
                                .loader()
                                .get_physical_device_surface_capabilities(self.physical_device, **self.surface)?
                };

                let requested_img_count = Self::clamp_image_count(self.desired_img_count, &surface_capabilities);

                let old_extent = self.extent;
                self.extent = Self::create_extent(&self.window, &surface_capabilities);
                recreation_info.extent_changed = old_extent != self.extent;

                self.viewport = Self::create_viewport(&self.extent);
                self.scissor = Self::create_scissor(&self.extent);
                debug!("VkSwapchain extent: {:?}", self.extent);

                let old_present_mode = self.present_mode;
                self.present_mode = Self::choose_present_mode(&self.surface, self.physical_device)?;
                if old_present_mode != self.present_mode {
                        debug!("VkSwapchain present mode: {:?}", self.present_mode);
                }

                let loader = Swapchain::new(&**self.instance, &**self.device);

                let swch_cinfo = vk::SwapchainCreateInfoKHR {
                        surface: **self.surface,
                        min_image_count: requested_img_count,
                        image_format: self.color_format.format,
                        image_color_space: self.color_format.color_space,
                        image_extent: self.extent,
                        image_array_layers: 1,
                        image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                        image_sharing_mode: vk::SharingMode::EXCLUSIVE,
                        queue_family_index_count: 0,
                        p_queue_family_indices: std::ptr::null(),
                        pre_transform: surface_capabilities.current_transform,
                        composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
                        present_mode: self.present_mode,
                        clipped: vk::TRUE,
                        old_swapchain: self.handle,
                        ..Default::default()
                };

                let old_handle = self.handle;
                self.handle = unsafe { loader.create_swapchain(&swch_cinfo, None)? };
                unsafe { self.loader.destroy_swapchain(old_handle, None) };

                let old_samples = self.samples;
                self.samples = Self::choose_sample_count(&self.instance, self.physical_device);
                recreation_info.samples_changed = old_samples != self.samples;
                if recreation_info.samples_changed {
                        debug!("VkSwapchain samples: {:?}", self.samples);
                }

                if recreation_info.color_format_changed
                        || recreation_info.extent_changed
                        || recreation_info.samples_changed
                {
                        let (color_img, color_img_view) = Self::create_color_img_resources(
                                Rc::clone(&self.device),
                                Rc::clone(&self.allocator),
                                self.color_format.format,
                                &self.extent,
                                self.samples,
                        )?;
                        unsafe {
                                std::mem::replace(&mut self.color_img, color_img).destroy();
                                std::mem::replace(&mut self.color_img_view, color_img_view).destroy();
                        }

                        let (depth_img, depth_img_view) = Self::create_depth_img_resources(
                                Rc::clone(&self.device),
                                Rc::clone(&self.allocator),
                                depth_format,
                                &self.extent,
                                self.samples,
                        )?;
                        unsafe {
                                std::mem::replace(&mut self.depth_img, depth_img).destroy();
                                std::mem::replace(&mut self.depth_img_view, depth_img_view).destroy();
                        }
                }

                self.resolve_imgs = unsafe { loader.get_swapchain_images(self.handle)? };
                unsafe { self.resolve_img_views.drain(..).for_each(|iv| iv.destroy()) };
                self.resolve_img_views = Self::create_resolve_img_views(
                        Rc::clone(&self.device),
                        &self.resolve_imgs,
                        self.color_format.format,
                )?;

                let old_img_count = self.img_count;
                self.img_count = self.resolve_imgs.len() as u32;
                recreation_info.img_count_changed = old_img_count != self.img_count;
                if recreation_info.img_count_changed {
                        debug!("VkSwapchain image count: {}", self.img_count);
                }

                Ok(recreation_info)
        }

        pub fn create_framebuffers(&mut self, render_pass: vk::RenderPass) -> VkResult<()> {
                assert!(self.framebuffers.is_empty());

                self.framebuffers = self
                        .resolve_img_views
                        .iter()
                        .map(|resolve_img_view| {
                                let attachments = [*self.color_img_view, *self.depth_img_view, **resolve_img_view];

                                let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                        .render_pass(render_pass)
                                        .attachments(&attachments)
                                        .width(self.extent.width)
                                        .height(self.extent.height)
                                        .layers(1);

                                unsafe { VkFramebuffer::new(&self.device, &framebuffer_cinfo) }
                        })
                        .collect::<VkResult<Vec<VkFramebuffer>>>()?;

                Ok(())
        }

        pub unsafe fn acquire_next_image(
                &self,
                timeout: u64,
                semaphore: vk::Semaphore,
                fence: vk::Fence,
        ) -> VkResult<(u32, bool)> {
                self.loader.acquire_next_image(self.handle, timeout, semaphore, fence)
        }

        pub unsafe fn queue_present(&self, queue: vk::Queue, present_info: &vk::PresentInfoKHR) -> VkResult<bool> {
                self.loader.queue_present(queue, present_info)
        }

        fn choose_color_format(
                surface: &VkSurface,
                physical_device: vk::PhysicalDevice,
        ) -> VkResult<vk::SurfaceFormatKHR> {
                let formats = unsafe {
                        surface.loader()
                                .get_physical_device_surface_formats(physical_device, **surface)?
                };

                let find_format = |fmt: vk::Format, color_space: vk::ColorSpaceKHR| {
                        formats.iter().find(|f| f.format == fmt && f.color_space == color_space)
                };

                if let Some(&fmt) = find_format(vk::Format::B8G8R8A8_UNORM, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        Ok(fmt)
                } else if let Some(&fmt) = find_format(vk::Format::B8G8R8A8_SRGB, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        Ok(fmt)
                } else {
                        Ok(formats[0])
                }
        }

        fn clamp_image_count(image_count: u32, surface_capabilities: &vk::SurfaceCapabilitiesKHR) -> u32 {
                image_count.clamp(
                        surface_capabilities.min_image_count,
                        match surface_capabilities.max_image_count {
                                0 => u32::MAX,
                                _ => surface_capabilities.max_image_count,
                        },
                )
        }

        fn create_extent(
                window: &winit::window::Window,
                surface_capabilities: &vk::SurfaceCapabilitiesKHR,
        ) -> vk::Extent2D {
                match surface_capabilities.current_extent.width {
                        u32::MAX => vk::Extent2D {
                                width: window.inner_size().width,
                                height: window.inner_size().height,
                        },
                        _ => surface_capabilities.current_extent,
                }
        }

        fn create_viewport(extent: &vk::Extent2D) -> vk::Viewport {
                vk::Viewport {
                        x: 0.0,
                        y: extent.height as f32,
                        width: extent.width as f32,
                        height: -(extent.height as f32),
                        min_depth: 0.0,
                        max_depth: 1.0,
                }
        }

        fn create_scissor(extent: &vk::Extent2D) -> vk::Rect2D {
                vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: *extent,
                }
        }

        fn choose_present_mode(
                surface: &VkSurface,
                physical_device: vk::PhysicalDevice,
        ) -> VkResult<vk::PresentModeKHR> {
                let modes = unsafe {
                        surface.loader()
                                .get_physical_device_surface_present_modes(physical_device, **surface)?
                };

                let is_present_mode_avail = |mode: vk::PresentModeKHR| modes.iter().any(|&m| m == mode);

                if is_present_mode_avail(vk::PresentModeKHR::MAILBOX) {
                        Ok(vk::PresentModeKHR::MAILBOX)
                } else if is_present_mode_avail(vk::PresentModeKHR::IMMEDIATE) {
                        Ok(vk::PresentModeKHR::IMMEDIATE)
                } else {
                        Ok(vk::PresentModeKHR::FIFO)
                }
        }

        fn choose_sample_count(instance: &ash::Instance, physical_device: vk::PhysicalDevice) -> vk::SampleCountFlags {
                let limits = unsafe { instance.get_physical_device_properties(physical_device).limits };

                let avail_samples = limits.framebuffer_color_sample_counts & limits.framebuffer_depth_sample_counts;

                if avail_samples.contains(vk::SampleCountFlags::TYPE_64) {
                        vk::SampleCountFlags::TYPE_64
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_32) {
                        vk::SampleCountFlags::TYPE_32
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_16) {
                        vk::SampleCountFlags::TYPE_16
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_8) {
                        vk::SampleCountFlags::TYPE_8
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_4) {
                        vk::SampleCountFlags::TYPE_4
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_2) {
                        vk::SampleCountFlags::TYPE_2
                } else {
                        vk::SampleCountFlags::TYPE_1
                }
        }

        fn create_color_img_resources(
                device: Rc<VkDevice>,
                allocator: Rc<VmaAllocator>,
                format: vk::Format,
                extent: &vk::Extent2D,
                samples: vk::SampleCountFlags,
        ) -> VkResult<(VkImage, VkImageView)> {
                let color_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                flags: Default::default(),
                                image_type: vk::ImageType::TYPE_2D,
                                format,
                                extent: vk::Extent3D {
                                        width: extent.width,
                                        height: extent.height,
                                        depth: 1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::TRANSIENT_ATTACHMENT
                                        | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::NONE,
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(allocator, &depth_img_cinfo)?
                };

                let color_img_view = unsafe {
                        let color_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *color_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &color_img_view_cinfo)?
                };

                Ok((color_img, color_img_view))
        }

        fn create_depth_img_resources(
                device: Rc<VkDevice>,
                allocator: Rc<VmaAllocator>,
                format: vk::Format,
                extent: &vk::Extent2D,
                samples: vk::SampleCountFlags,
        ) -> VkResult<(VkImage, VkImageView)> {
                let depth_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                flags: Default::default(),
                                image_type: vk::ImageType::TYPE_2D,
                                format,
                                extent: vk::Extent3D {
                                        width: extent.width,
                                        height: extent.height,
                                        depth: 1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::NONE,
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(allocator, &depth_img_cinfo)?
                };

                let depth_img_view = unsafe {
                        let depth_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *depth_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: vk::Format::D24_UNORM_S8_UINT,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::DEPTH,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &depth_img_view_cinfo)?
                };

                Ok((depth_img, depth_img_view))
        }

        fn create_resolve_img_views(
                device: Rc<VkDevice>,
                resolve_imgs: &[vk::Image],
                format: vk::Format,
        ) -> VkResult<Vec<VkImageView>> {
                resolve_imgs
                        .iter()
                        .map(|&image| {
                                let img_view_cinfo = vk::ImageViewCreateInfo {
                                        image,
                                        view_type: vk::ImageViewType::TYPE_2D,
                                        format,
                                        components: vk::ComponentMapping::default(),
                                        subresource_range: vk::ImageSubresourceRange {
                                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                                base_mip_level: 0,
                                                level_count: 1,
                                                base_array_layer: 0,
                                                layer_count: 1,
                                        },
                                        ..Default::default()
                                };

                                unsafe { VkImageView::new(Rc::clone(&device), &img_view_cinfo) }
                        })
                        .collect::<VkResult<Vec<VkImageView>>>()
        }
}

impl_destroyable_expr!(VkSwapchain, vk::SwapchainKHR, |s: &VkSwapchain| {
        s.resolve_img_views.iter().for_each(|iv| iv.destroy());
        s.framebuffers.iter().for_each(|fb| fb.destroy());
        s.depth_img_view.destroy();
        s.depth_img.destroy();
        s.color_img_view.destroy();
        s.color_img.destroy();
        s.loader.destroy_swapchain(s.handle, None)
});

bitflags! {
        pub struct VkSwapchainOutdatedCauseFlags: u32 {
                const NONE = 0b00000000;
                const WINDOW_RESIZE = 0b00000001;
                const SUBOPTIMAL = 0b00000010;
                const OUT_OF_DATE = 0b00000100;
        }
}

pub struct VkSwapchainRecreationInfo {
        pub color_format_changed: bool,
        pub extent_changed: bool,
        pub samples_changed: bool,
        pub img_count_changed: bool,
}
