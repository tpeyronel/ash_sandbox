use hashbrown::HashMap;

use crate::{
        asset_manager::{AssetManagerEvent, Observable},
        shader_resource::{ShaderResource, ShaderResourceId, ShaderResourceProvider, ShaderResourceType, ShaderStruct},
};

#[derive(Debug)]
pub struct ShaderResourceRegistry {
        registers: Observable<
                ShaderResourceId,
                ShaderResource,
                AssetManagerEvent,
                HashMap<ShaderResourceId, ShaderResource>,
        >,
}

impl ShaderResourceRegistry {
        pub fn new(event_tx: crossbeam_channel::Sender<AssetManagerEvent>) -> Self {
                Self {
                        registers: Observable::new(HashMap::new(), event_tx),
                }
        }

        pub fn register(&mut self, shader_resource: ShaderResource) {
                let old = self.registers.insert(shader_resource.id.clone(), shader_resource);

                if let Some(old) = old {
                        panic!("duplicate shader resource id {}", &old.id);
                }
        }

        pub fn register_struct<T: ShaderStruct>(&mut self, id: ShaderResourceId, provider: ShaderResourceProvider) {
                self.register(ShaderResource {
                        id,
                        resource_type: ShaderResourceType::Struct(T::shader_struct_declaration()),
                        provider,
                })
        }

        pub fn get(&self, shader_resource_id: &ShaderResourceId) -> Option<&ShaderResource> {
                self.registers.get(shader_resource_id)
        }
}
