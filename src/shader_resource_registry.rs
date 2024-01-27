use hashbrown::HashMap;
use thiserror::Error;

use crate::asset_manager::{ShaderResource, ShaderResourceId};

#[derive(Error, Debug)]
pub enum ShaderResourceRegisterError {
        #[error("another shader resource with the same resource id already exists: {0}")]
        DuplicateShaderResourceId(String),
}

#[derive(Debug)]
pub struct ShaderResourceRegistry {
        registers: HashMap<ShaderResourceId, ShaderResource>,
}

impl ShaderResourceRegistry {
        pub fn new() -> Self {
                Self {
                        registers: HashMap::new(),
                }
        }

        pub fn register(&mut self, shader_resource: ShaderResource) -> Result<(), ShaderResourceRegisterError> {
                if self.registers.contains_key(&shader_resource.id) {
                        return Err(ShaderResourceRegisterError::DuplicateShaderResourceId(
                                shader_resource.id,
                        ));
                }

                self.registers.insert(shader_resource.id.clone(), shader_resource);

                Ok(())
        }

        pub fn get(&self, shader_resource_id: &ShaderResourceId) -> Option<&ShaderResource> {
                self.registers.get(shader_resource_id)
        }
}
