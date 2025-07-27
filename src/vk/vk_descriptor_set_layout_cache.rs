use std::rc::Rc;

use ash::{prelude::VkResult, vk};

use crate::hashmap::HashMap;

use super::vk_wrapper::VkDevice;

pub struct VkDescriptorSetLayoutCache {
        device: Rc<VkDevice>,
        layouts: HashMap<LayoutKey, vk::DescriptorSetLayout>,
}

impl VkDescriptorSetLayoutCache {
        pub fn new(device: Rc<VkDevice>) -> Self {
                Self {
                        device,
                        layouts: HashMap::new(),
                }
        }

        pub unsafe fn create_layout(
                &mut self,
                mut bindings: Vec<vk::DescriptorSetLayoutBinding<'static>>,
        ) -> VkResult<vk::DescriptorSetLayout> {
                bindings.sort_by_key(|b| b.binding);

                let layout_key = LayoutKey(bindings);
                let layout = match self.layouts.get(&layout_key) {
                        Some(&layout) => layout,
                        None => {
                                let layout_cinfo = vk::DescriptorSetLayoutCreateInfo::default().bindings(&layout_key.0);
                                let new_layout = self.device.create_descriptor_set_layout(&layout_cinfo, None)?;
                                self.layouts.insert(layout_key, new_layout);
                                new_layout
                        },
                };

                Ok(layout)
        }

        pub unsafe fn destroy(&mut self) {
                for (_, layout) in self.layouts.drain() {
                        self.device.destroy_descriptor_set_layout(layout, None);
                }
        }
}

struct LayoutKey(Vec<vk::DescriptorSetLayoutBinding<'static>>);

impl PartialEq for LayoutKey {
        fn eq(&self, other: &Self) -> bool {
                if self.0.len() != other.0.len() {
                        return false;
                }

                for (l, r) in self.0.iter().zip(other.0.iter()) {
                        if l.descriptor_type != r.descriptor_type
                                || l.descriptor_count != r.descriptor_count
                                || l.stage_flags != r.stage_flags
                                || l.binding != r.binding
                                || l.p_immutable_samplers.is_null()
                                || r.p_immutable_samplers.is_null()
                        {
                                return false;
                        }
                }

                true
        }
}

impl Eq for LayoutKey {}

impl std::hash::Hash for LayoutKey {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                for binding in &self.0 {
                        binding.binding.hash(state);
                        binding.descriptor_type.hash(state);
                        binding.descriptor_count.hash(state);
                        binding.stage_flags.hash(state);
                }
        }
}
