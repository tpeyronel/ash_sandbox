use std::{ops::Deref, sync::Arc};

use ash::vk;
use log::trace;

use crate::vkma_error::VkmaResult;

pub struct VkImageCreateInfo {
        pub image_type:           vk::ImageType,
        pub format:               vk::Format,
        pub extent:               vk::Extent3D,
        pub mip_levels:           u32,
        pub array_layers:         u32,
        pub samples:              vk::SampleCountFlags,
        pub tiling:               vk::ImageTiling,
        pub usage:                vk::ImageUsageFlags,
        pub queue_family_indices: Option<Vec<u32>>,
        pub initial_layout:       vk::ImageLayout,

        pub mem_usage:       vma::MemoryUsage,
        pub alloc_cflags:    vma::AllocationCreateFlags,
        pub required_flags:  vk::MemoryPropertyFlags,
        pub preferred_flags: vk::MemoryPropertyFlags,
}

pub struct VkImage {
        allocator: Arc<vma::Allocator>,

        handle: vk::Image,
        alloc:  vma::Allocation,
        ainfo:  vma::AllocationInfo,
}

impl VkImage {
        pub unsafe fn new(allocator: &Arc<vma::Allocator>, create_info: &VkImageCreateInfo) -> VkmaResult<Self> {
                let (handle, alloc, ainfo) = {
                        let mut vk_img_cinfo = vk::ImageCreateInfo {
                                image_type: create_info.image_type,
                                format: create_info.format,
                                extent: create_info.extent,
                                mip_levels: create_info.mip_levels,
                                array_layers: create_info.array_layers,
                                samples: create_info.samples,
                                tiling: create_info.tiling,
                                usage: create_info.usage,
                                initial_layout: create_info.initial_layout,
                                ..vk::ImageCreateInfo::default()
                        };

                        match &create_info.queue_family_indices {
                                Some(queue_families_indices) => {
                                        assert!(!queue_families_indices.is_empty());

                                        vk_img_cinfo.sharing_mode = vk::SharingMode::CONCURRENT;
                                        vk_img_cinfo.p_queue_family_indices = queue_families_indices.as_ptr();
                                        vk_img_cinfo.queue_family_index_count = queue_families_indices.len() as u32;
                                },
                                None => {
                                        vk_img_cinfo.sharing_mode = vk::SharingMode::EXCLUSIVE;
                                        vk_img_cinfo.p_queue_family_indices = std::ptr::null();
                                        vk_img_cinfo.queue_family_index_count = 0;
                                },
                        }

                        let alloc_cinfo = vma::AllocationCreateInfo {
                                usage:            create_info.mem_usage,
                                flags:            create_info.alloc_cflags,
                                required_flags:   create_info.required_flags,
                                preferred_flags:  create_info.preferred_flags,
                                memory_type_bits: 0,
                                pool:             None,
                                user_data:        None,
                        };

                        allocator.create_image(&vk_img_cinfo, &alloc_cinfo)?
                };

                Ok(Self {
                        allocator: Arc::clone(allocator),
                        handle,
                        alloc,
                        ainfo,
                })
        }
}

impl Deref for VkImage {
        type Target = vk::Image;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkImage {
        fn drop(&mut self) {
                trace!("Destroying VkImage...");

                assert_ne!(self.handle, vk::Image::null());

                let _ = self.allocator.destroy_image(self.handle, &self.alloc);

                self.handle = vk::Image::null();
        }
}
