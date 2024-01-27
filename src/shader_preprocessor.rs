use lazy_regex::{lazy_regex, Lazy};
use regex::Regex;

use crate::{
        asset_manager::{ShaderLoadError, ShaderResourceId, ShaderResourceType},
        hashmap::HashMap,
        shader_resource::{ShaderStructDeclaration, ShaderStructFieldType},
        shader_resource_registry::ShaderResourceRegistry,
};

static DIRECTIVE_RESOURCE_REGEX: Lazy<Regex> = lazy_regex!(
        r"^#resource\s+([a-zA-Z_][a-zA-Z_0-9]*)\s+([a-zA-Z_][a-zA-Z_0-9]*)\s+:\s+([a-zA-Z_][a-zA-Z_0-9]*)\s*;\s*$"
);

pub struct ShaderPreprocessor {}

impl ShaderPreprocessor {
        pub fn preprocess_glsl_source(
                shader_resources: &ShaderResourceRegistry,
                source_code: &str,
        ) -> Result<PreprocessedShaderStage, ShaderLoadError> {
                let mut parts = vec![String::new()];
                let mut resources = vec![];
                let mut declared_structs: HashMap<String, ShaderStructDeclaration> = HashMap::new();

                for line in source_code.lines() {
                        let line = line.to_string() + "\n";

                        let part = parts.last_mut().unwrap();

                        if !line.starts_with("#") {
                                *part += &line;
                                continue;
                        }

                        let Some(directive) = Self::parse_directive(shader_resources, &line)? else {
                                *part += &line;
                                continue;
                        };

                        match directive {
                                ShaderDirective::Resource {
                                        resource_id,
                                        variable_name,
                                } => {
                                        let resource = shader_resources.get(&resource_id).unwrap();

                                        if let ShaderResourceType::Struct(declaration) = &resource.resource_type {
                                                Self::write_required_struct_declarations_rec(
                                                        &mut declared_structs,
                                                        declaration,
                                                        part,
                                                );
                                        }

                                        resources.push(ShaderResourceRequirement {
                                                resource_id,
                                                variable_name,
                                                separator_index: parts.len() - 1,
                                        });

                                        parts.push(String::new());
                                },
                        }
                }

                let parts = ShaderStageParts(parts);
                let preprocessed_shader = PreprocessedShaderStage { parts, resources };

                Ok(preprocessed_shader)
        }

        fn parse_directive(
                shader_resources: &ShaderResourceRegistry,
                line: &str,
        ) -> Result<Option<ShaderDirective>, ShaderLoadError> {
                let directive_type = line
                        .find(|c: char| c.is_whitespace())
                        .map(|i| &line[1..i])
                        .unwrap_or(&line);

                let directive = match directive_type {
                        "resource" => {
                                let captures = DIRECTIVE_RESOURCE_REGEX
                                        .captures(&line)
                                        .ok_or(ShaderLoadError::InvalidPreprocessorDirective("resource"))?;

                                let type_name = captures.get(1).unwrap().as_str().to_string();
                                let variable_name = captures.get(2).unwrap().as_str().to_string();
                                let resource_id = captures.get(3).unwrap().as_str().to_string();

                                let resource = shader_resources
                                        .get(&resource_id)
                                        .ok_or_else(|| ShaderLoadError::UnknownShaderResourceId(resource_id.clone()))?;

                                assert_eq!(resource.resource_type.glsl_type_name(), type_name);

                                ShaderDirective::Resource {
                                        resource_id,
                                        variable_name,
                                }
                        },
                        _ => return Ok(None),
                };

                Ok(Some(directive))
        }

        fn write_required_struct_declarations_rec(
                declared_structs: &mut HashMap<String, ShaderStructDeclaration>,
                declaration: &ShaderStructDeclaration,
                out: &mut String,
        ) {
                for f in &declaration.fields {
                        let ShaderStructFieldType::Struct(child_declaration) = &f.field_type else {
                                continue;
                        };

                        Self::write_required_struct_declarations_rec(declared_structs, child_declaration, out);

                        if let Some(declared) = declared_structs.get(&child_declaration.type_name) {
                                assert_eq!(declared, child_declaration); // Check that expected type is correct.
                                continue;
                        }

                        declared_structs.insert(child_declaration.type_name.clone(), child_declaration.clone());

                        *out += &child_declaration.glsl_type_declaration();
                }
        }
}

#[derive(Debug)]
pub enum ShaderDirective {
        Resource {
                resource_id: ShaderResourceId,
                variable_name: String,
        },
}

#[derive(Debug, Clone)]
pub struct ShaderResourceRequirement {
        pub resource_id: ShaderResourceId,
        pub variable_name: String,
        pub separator_index: usize,
}

#[derive(Debug, Clone)]
pub struct PreprocessedShaderStage {
        pub parts: ShaderStageParts,
        pub resources: Vec<ShaderResourceRequirement>,
}

#[derive(Debug, Clone)]
pub struct ShaderStageParts(Vec<String>);

pub struct ShaderStageSourceBuilder {
        parts: Vec<String>,
        separators: Vec<Option<String>>,
}

impl ShaderStageSourceBuilder {
        pub fn new(parts: &ShaderStageParts) -> Self {
                assert!(!parts.0.is_empty());

                Self {
                        parts: parts.0.clone(),
                        separators: vec![None; parts.0.len() - 1],
                }
        }

        pub fn place(&mut self, index: usize, separator: String) {
                let f = self.separators.get_mut(index).expect("invalid separator index");
                assert_eq!(*f, None, "separator at {} already exists", index);

                *f = Some(separator);
        }

        pub fn build(&self) -> String {
                let mut result = String::new();

                result += &self.parts[0]; // new() ensures self.parts is not empty
                for i in 1..self.parts.len() {
                        result += self.separators[i - 1]
                                .as_ref()
                                .expect(&format!("shader hole {} wasn't filled", i));
                        result += &self.parts[i];
                }

                result
        }
}
