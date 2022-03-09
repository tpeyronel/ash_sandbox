use std::rc::Rc;

use ash::{prelude::VkResult, vk};

use super::vk_wrapper::VkDevice;

pub struct VkDescriptorSetAllocator {
        device: Rc<VkDevice>,
        dst_pools: Vec<vk::DescriptorPool>,
        pool_index: usize,
}

impl VkDescriptorSetAllocator {
        pub fn new(device: Rc<VkDevice>) -> VkResult<Self> {
                let dst_pool = Self::create_dst_pool(&device)?;

                Ok(Self {
                        device,
                        dst_pools: vec![dst_pool],
                        pool_index: 0,
                })
        }

        pub unsafe fn allocate_descriptor_sets<const N: usize>(
                &mut self,
                dst_set_layouts: &[vk::DescriptorSetLayout; N],
        ) -> VkResult<[vk::DescriptorSet; N]> {
                self.allocate_descriptor_sets_inner(dst_set_layouts, true)
        }

        unsafe fn allocate_descriptor_sets_inner<const N: usize>(
                &mut self,
                dst_set_layouts: &[vk::DescriptorSetLayout; N],
                retry: bool,
        ) -> VkResult<[vk::DescriptorSet; N]> {
                let create_info = vk::DescriptorSetAllocateInfo::builder()
                        .descriptor_pool(self.dst_pools[self.pool_index])
                        .set_layouts(dst_set_layouts);

                let dst_sets: [vk::DescriptorSet; N] = match self.device.allocate_descriptor_sets(&create_info) {
                        Ok(dst_sets) => dst_sets.try_into().unwrap(),
                        Err(vk_result) => match vk_result {
                                vk::Result::ERROR_OUT_OF_POOL_MEMORY | vk::Result::ERROR_FRAGMENTED_POOL if retry => {
                                        log::warn!(
                                                "DESCRIPTOR POOL ERROR ({:?}). Allocating new descriptor pool.",
                                                vk_result
                                        );

                                        if self.dst_pools.len() == (self.pool_index + 1) {
                                                self.dst_pools.push(Self::create_dst_pool(&self.device)?);
                                        }
                                        self.pool_index += 1;

                                        self.allocate_descriptor_sets_inner(&dst_set_layouts, false)?
                                },
                                _ => return Err(vk_result),
                        },
                };

                Ok(dst_sets)
        }

        pub unsafe fn reset_pools(&mut self) -> VkResult<()> {
                for &dst_pool in &self.dst_pools {
                        self.device
                                .reset_descriptor_pool(dst_pool, vk::DescriptorPoolResetFlags::empty())?;
                }

                self.pool_index = 0;

                Ok(())
        }

        pub unsafe fn destroy(&mut self) {
                assert!(!self.dst_pools.is_empty());

                for dst_pool in self.dst_pools.drain(..) {
                        self.device.destroy_descriptor_pool(dst_pool, None)
                }
        }

        fn create_dst_pool(device: &VkDevice) -> VkResult<vk::DescriptorPool> {
                let pool_sizes = [
                        pool_size(vk::DescriptorType::SAMPLER, 100),
                        pool_size(vk::DescriptorType::COMBINED_IMAGE_SAMPLER, 100),
                        pool_size(vk::DescriptorType::SAMPLED_IMAGE, 100),
                        pool_size(vk::DescriptorType::STORAGE_IMAGE, 100),
                        pool_size(vk::DescriptorType::UNIFORM_BUFFER, 100),
                        pool_size(vk::DescriptorType::STORAGE_BUFFER, 100),
                        pool_size(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, 100),
                        pool_size(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC, 100),
                        pool_size(vk::DescriptorType::UNIFORM_TEXEL_BUFFER, 10),
                        pool_size(vk::DescriptorType::STORAGE_TEXEL_BUFFER, 10),
                        pool_size(vk::DescriptorType::INPUT_ATTACHMENT, 10),
                ];

                let dst_pool_cinfo = vk::DescriptorPoolCreateInfo::builder()
                        .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                        .pool_sizes(&pool_sizes)
                        .max_sets(1000);

                unsafe { device.create_descriptor_pool(&dst_pool_cinfo, None) }
        }
}

fn pool_size(ty: vk::DescriptorType, count: u32) -> vk::DescriptorPoolSize {
        vk::DescriptorPoolSize {
                ty,
                descriptor_count: count,
        }
}
