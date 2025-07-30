use std::{cell::Cell, ops::Deref, rc::Rc};

use ash::{khr::swapchain, prelude::VkResult, vk};
use bitflags::bitflags;
#[allow(unused_imports)]
use log::{debug, trace};

use crate::{
        vk::{
                vk_context::VkContext,
                vk_wrapper::{HasVkHandle, VkSemaphore},
        },
        AnyResult,
};

use super::{
        vk_image::{VkImage, VkImageCreateInfo},
        vk_util,
        vk_wrapper::{impl_destroyable_deref, impl_destroyable_drop, impl_destroyable_expr, VkImageView, VkSurface},
};

pub struct VkSwapchain {
        device_loader: swapchain::Device,

        window: Rc<winit::window::Window>,
        desired_img_count: u32,

        handle: vk::SwapchainKHR,
        destroyed: Cell<bool>,

        pub color_format: vk::Format,
        pub depth_format: vk::Format,
        pub present_format: vk::SurfaceFormatKHR,
        pub extent: vk::Extent2D,
        pub viewport: vk::Viewport,
        pub scissor: vk::Rect2D,
        pub present_mode: vk::PresentModeKHR,
        pub samples: vk::SampleCountFlags,

        pub color_img: VkImage,
        pub color_img_view: VkImageView,

        pub depth_img: VkImage,
        pub depth_img_view: VkImageView,

        pub resolve_imgs: [VkImage; 2],
        pub resolve_img_views: [VkImageView; 2],

        pub present_imgs: Vec<VkPresentImageData>,
}

impl VkSwapchain {
        pub fn new(window: Rc<winit::window::Window>, context: &VkContext, desired_img_count: u32) -> AnyResult<Self> {
                let color_format = Self::choose_color_format(context)?;
                let depth_format = Self::choose_depth_format(context)?;
                let present_format = Self::choose_present_format(context)?;
                debug!("VkSwapchain present format ({:?})", present_format);

                let surface_capabilities = context.get_physical_device_surface_capabilities()?;

                let requested_img_count = Self::clamp_image_count(desired_img_count, &surface_capabilities);
                debug!("VkSwapchain image count: {}", requested_img_count);

                let extent = Self::create_extent(&window, &surface_capabilities);
                debug!("VkSwapchain extent: {:?}", extent);

                let viewport = Self::create_viewport(&extent);
                let scissor = Self::create_scissor(&extent);

                let pre_transform = surface_capabilities.current_transform;

                let present_mode = Self::choose_present_mode(context)?;
                debug!("VkSwapchain present mode: {:?}", present_mode);

                let device_loader = swapchain::Device::new(&context.instance, &context.device);

                let swch_cinfo = Self::swapchain_create_info(
                        &context.surface,
                        requested_img_count,
                        present_format,
                        extent,
                        pre_transform,
                        present_mode,
                        vk::SwapchainKHR::null(),
                );

                let handle = unsafe { device_loader.create_swapchain(&swch_cinfo, None)? };

                let samples = Self::choose_sample_count(context);
                debug!("Swapchain samples: {:?}", samples);

                let (color_img, color_img_view) =
                        Self::create_color_img_resources(context, color_format, &extent, samples)?;
                let (resolve_imgs, resolve_img_views) =
                        Self::create_resolve_imgs_resources(context, color_format, &extent)?;
                let (depth_img, depth_img_view) =
                        Self::create_depth_img_resources(context, depth_format, &extent, samples)?;

                let present_imgs =
                        Self::create_present_img_data(context, handle, &device_loader, present_format.format)?;

                Ok(Self {
                        device_loader,

                        window,
                        desired_img_count,

                        handle,
                        destroyed: Cell::new(false),

                        color_format,
                        depth_format,
                        present_format,

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

                        present_imgs,
                })
        }

        pub fn recreate(&mut self, context: &VkContext) -> VkResult<VkSwapchainRecreationInfo> {
                let mut recreation_info = VkSwapchainRecreationInfo {
                        present_format_changed: false,
                        extent_changed: false,
                        samples_changed: false,
                        img_count_changed: false,
                };

                let old_present_format = self.present_format;
                self.present_format = Self::choose_present_format(context)?;
                recreation_info.present_format_changed = old_present_format != self.present_format;
                if recreation_info.present_format_changed {
                        debug!("VkSwapchain present format ({:?})", self.color_format);
                }

                self.depth_format = Self::choose_depth_format(context)?;

                let surface_capabilities = context.get_physical_device_surface_capabilities()?;

                let requested_img_count = Self::clamp_image_count(self.desired_img_count, &surface_capabilities);

                let old_extent = self.extent;
                self.extent = Self::create_extent(&self.window, &surface_capabilities);
                recreation_info.extent_changed = old_extent != self.extent;

                self.viewport = Self::create_viewport(&self.extent);
                self.scissor = Self::create_scissor(&self.extent);
                debug!("VkSwapchain extent: {:?}", self.extent);

                let old_present_mode = self.present_mode;
                self.present_mode = Self::choose_present_mode(context)?;
                if old_present_mode != self.present_mode {
                        debug!("VkSwapchain present mode: {:?}", self.present_mode);
                }

                let swch_cinfo = Self::swapchain_create_info(
                        &context.surface,
                        requested_img_count,
                        self.present_format,
                        self.extent,
                        surface_capabilities.current_transform,
                        self.present_mode,
                        self.handle,
                );

                unsafe {
                        let old_handle = std::mem::replace(
                                &mut self.handle,
                                self.device_loader.create_swapchain(&swch_cinfo, None)?,
                        );
                        // Destroy old swapchain _after_ creating the new one.
                        self.device_loader.destroy_swapchain(old_handle, None);
                }

                let old_samples = self.samples;
                self.samples = Self::choose_sample_count(context);
                recreation_info.samples_changed = old_samples != self.samples;
                if recreation_info.samples_changed {
                        debug!("VkSwapchain samples: {:?}", self.samples);
                }

                // This doesn't make much sense i think.
                if recreation_info.present_format_changed
                        || recreation_info.extent_changed
                        || recreation_info.samples_changed
                {
                        let (color_img, color_img_view) = Self::create_color_img_resources(
                                context,
                                self.color_format,
                                &self.extent,
                                self.samples,
                        )?;
                        unsafe {
                                std::mem::replace(&mut self.color_img, color_img).destroy();
                                std::mem::replace(&mut self.color_img_view, color_img_view).destroy();
                        }

                        let (resolve_imgs, resolve_img_views) =
                                Self::create_resolve_imgs_resources(&context, self.color_format, &self.extent)?;
                        unsafe {
                                std::mem::replace(&mut self.resolve_imgs, resolve_imgs)
                                        .iter()
                                        .for_each(|x| x.destroy());
                                std::mem::replace(&mut self.resolve_img_views, resolve_img_views)
                                        .iter()
                                        .for_each(|x| x.destroy());
                        }

                        let (depth_img, depth_img_view) = Self::create_depth_img_resources(
                                context,
                                self.depth_format,
                                &self.extent,
                                self.samples,
                        )?;
                        unsafe {
                                std::mem::replace(&mut self.depth_img, depth_img).destroy();
                                std::mem::replace(&mut self.depth_img_view, depth_img_view).destroy();
                        }
                }

                let old_img_count = self.present_imgs.len();

                self.present_imgs.drain(..).for_each(|i| unsafe { i.destroy() });
                self.present_imgs = Self::create_present_img_data(
                        context,
                        self.handle,
                        &self.device_loader,
                        self.present_format.format,
                )?;

                let new_img_count = self.present_imgs.len();
                recreation_info.img_count_changed = old_img_count != new_img_count;
                if recreation_info.img_count_changed {
                        debug!("VkSwapchain image count: {}", new_img_count);
                }

                Ok(recreation_info)
        }

        pub unsafe fn acquire_next_image(
                &self,
                timeout: u64,
                semaphore: vk::Semaphore,
                fence: vk::Fence,
        ) -> VkResult<(u32, bool)> {
                self.device_loader
                        .acquire_next_image(self.handle, timeout, semaphore, fence)
        }

        pub unsafe fn queue_present(&self, queue: vk::Queue, present_info: &vk::PresentInfoKHR) -> VkResult<bool> {
                self.device_loader.queue_present(queue, present_info)
        }

        fn choose_color_format(context: &VkContext) -> VkResult<vk::Format> {
                let candidates = [vk::Format::R16G16B16A16_SFLOAT];

                let features = vk::FormatFeatureFlags::COLOR_ATTACHMENT | vk::FormatFeatureFlags::BLIT_SRC;

                vk_util::find_best_format_for_optimal_tiling(context, &candidates, features)
        }

        fn choose_depth_format(context: &VkContext) -> VkResult<vk::Format> {
                let candidates = [
                        vk::Format::D32_SFLOAT,
                        vk::Format::D32_SFLOAT_S8_UINT,
                        vk::Format::D24_UNORM_S8_UINT,
                        vk::Format::D16_UNORM,
                        vk::Format::D16_UNORM_S8_UINT,
                ];

                let features = vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT;

                vk_util::find_best_format_for_optimal_tiling(context, &candidates, features)
        }

        fn choose_present_format(context: &VkContext) -> VkResult<vk::SurfaceFormatKHR> {
                let formats = unsafe {
                        context.surface.instance_loader().get_physical_device_surface_formats(
                                context.pdevice.handle(),
                                context.surface.handle(),
                        )?
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

        fn choose_present_mode(context: &VkContext) -> VkResult<vk::PresentModeKHR> {
                let modes = unsafe {
                        context.surface
                                .instance_loader()
                                .get_physical_device_surface_present_modes(
                                        context.pdevice.handle(),
                                        context.surface.handle(),
                                )?
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

        fn swapchain_create_info(
                surface: &VkSurface,
                requested_img_count: u32,
                present_format: vk::SurfaceFormatKHR,
                extent: vk::Extent2D,
                pre_transform: vk::SurfaceTransformFlagsKHR,
                present_mode: vk::PresentModeKHR,
                old_swapchain: vk::SwapchainKHR,
        ) -> vk::SwapchainCreateInfoKHR {
                vk::SwapchainCreateInfoKHR::default()
                        .surface(**surface)
                        .min_image_count(requested_img_count)
                        .image_color_space(present_format.color_space)
                        .image_format(present_format.format)
                        .image_extent(extent)
                        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
                        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .pre_transform(pre_transform)
                        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                        .present_mode(present_mode)
                        .clipped(true)
                        .image_array_layers(1)
                        .old_swapchain(old_swapchain)
        }

        fn choose_sample_count(context: &VkContext) -> vk::SampleCountFlags {
                let limits = context.pdevice.props.limits;
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
                context: &VkContext,
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
                                usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::empty(),
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(context, &depth_img_cinfo)?
                };
                context.set_debug_name(&color_img, "render_color_image");

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

                        VkImageView::new(Rc::clone(&context.device), &color_img_view_cinfo)?
                };
                context.set_debug_name(&color_img_view, "render_color_image_view");

                Ok((color_img, color_img_view))
        }

        fn create_resolve_imgs_resources(
                context: &VkContext,
                format: vk::Format,
                extent: &vk::Extent2D,
        ) -> VkResult<([VkImage; 2], [VkImageView; 2])> {
                let mk_resolve_img = || {
                        unsafe {
                                let resolve_img_cinfo = VkImageCreateInfo {
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
                                        samples: vk::SampleCountFlags::TYPE_1,
                                        tiling: vk::ImageTiling::OPTIMAL,
                                        usage: vk::ImageUsageFlags::COLOR_ATTACHMENT
                                                | vk::ImageUsageFlags::SAMPLED
                                                | vk::ImageUsageFlags::TRANSFER_DST // for color -> resolve
                                                | vk::ImageUsageFlags::TRANSFER_SRC, // for resolve -> present
                                        queue_family_indices: None,
                                        initial_layout: vk::ImageLayout::UNDEFINED,

                                        mem_usage: vma::MemoryUsage::GpuOnly,
                                        alloc_cflags: vma::AllocationCreateFlags::empty(),
                                        required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                        preferred_flags: Default::default(),
                                };

                                VkImage::new(context, &resolve_img_cinfo)
                        }
                };

                let resolve_imgs = [mk_resolve_img()?, mk_resolve_img()?];

                resolve_imgs
                        .iter()
                        .enumerate()
                        .for_each(|(i, img)| context.set_debug_name(img, format!("resolve_image_{}", i)));

                let mk_resolve_img_view = |resolve_img: vk::Image| unsafe {
                        let resolve_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: resolve_img,
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

                        VkImageView::new(Rc::clone(&context.device), &resolve_img_view_cinfo)
                };

                let resolve_img_views = [
                        mk_resolve_img_view(*resolve_imgs[0])?,
                        mk_resolve_img_view(*resolve_imgs[1])?,
                ];

                resolve_img_views.iter().enumerate().for_each(|(i, img_view)| {
                        context.set_debug_name(img_view, format!("resolve_image_view_{}", i))
                });

                Ok((resolve_imgs, resolve_img_views))
        }

        fn create_depth_img_resources(
                context: &VkContext,
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
                                alloc_cflags: vma::AllocationCreateFlags::empty(),
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(context, &depth_img_cinfo)?
                };
                context.set_debug_name(&depth_img, "render_depth_image");

                let depth_img_view = unsafe {
                        let depth_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *depth_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format,
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

                        VkImageView::new(Rc::clone(&context.device), &depth_img_view_cinfo)?
                };
                context.set_debug_name(&depth_img_view, "render_depth_image_view");

                Ok((depth_img, depth_img_view))
        }

        fn create_present_img_data(
                context: &VkContext,
                handle: vk::SwapchainKHR,
                device_loader: &swapchain::Device,
                format: vk::Format,
        ) -> VkResult<Vec<VkPresentImageData>> {
                let present_imgs = unsafe { device_loader.get_swapchain_images(handle)? };

                present_imgs.iter().enumerate().for_each(|(i, &p)| {
                        context.set_debug_name(p, format!("present_image_{}", i));
                });

                present_imgs
                        .into_iter()
                        .enumerate()
                        .map(|(i, img)| {
                                let img_view_cinfo = vk::ImageViewCreateInfo {
                                        image: img,
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

                                let img_view =
                                        unsafe { VkImageView::new(Rc::clone(&context.device), &img_view_cinfo)? };
                                context.set_debug_name(&img_view, format!("present_image_view_{}", i));

                                let semaphore_cinfo = vk::SemaphoreCreateInfo::default();
                                let render_finished_semaphore =
                                        unsafe { VkSemaphore::new(&context.device, &semaphore_cinfo)? };
                                context.set_debug_name(
                                        &render_finished_semaphore,
                                        format!("render_finished_semaphore_{}", i),
                                );

                                Ok(VkPresentImageData {
                                        img,
                                        img_view,
                                        render_finished_semaphore,
                                })
                        })
                        .collect::<VkResult<Vec<VkPresentImageData>>>()
        }
}

impl_destroyable_expr!(VkSwapchain, vk::SwapchainKHR, |s: &VkSwapchain| {
        s.present_imgs.iter().for_each(|iv| iv.destroy());
        s.resolve_img_views.iter().for_each(|x| x.destroy());
        s.resolve_imgs.iter().for_each(|x| x.destroy());
        s.depth_img_view.destroy();
        s.depth_img.destroy();
        s.color_img_view.destroy();
        s.color_img.destroy();
        s.device_loader.destroy_swapchain(s.handle, None);
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
        pub present_format_changed: bool,
        pub extent_changed: bool,
        pub samples_changed: bool,
        pub img_count_changed: bool,
}

pub struct VkPresentImageData {
        pub img: vk::Image,
        pub img_view: VkImageView,
        // Signaled when all the rendering commands for this image have finished executing,
        // which means that the rendered image is now ready for presentation.
        pub render_finished_semaphore: VkSemaphore,
}

impl VkPresentImageData {
        unsafe fn destroy(&self) {
                self.img_view.destroy();
                self.render_finished_semaphore.destroy();
        }
}
