use std::{ops::Deref, path::PathBuf};

use lazy_regex::{lazy_regex, Lazy};
use regex::Regex;

use crate::{
        asset_manager::ShaderLoadError,
        hashmap::HashMap,
        shader_resource::{ShaderResourceId, ShaderResourceType, ShaderStructDeclaration, ShaderStructFieldType},
        shader_resource_registry::ShaderResourceRegistry,
};

static DIRECTIVE_RESOURCE_REGEX: Lazy<Regex> = lazy_regex!(
        r"^#resource\s+([a-zA-Z_][a-zA-Z_0-9]*(?:\[\])?)\s+([a-zA-Z_][a-zA-Z_0-9]*)\s+:\s+([a-zA-Z_][a-zA-Z_0-9]*)\s*;\s*$"
);

static DIRECTIVE_READONLY_RESOURCE_REGEX: Lazy<Regex> = lazy_regex!(
        r"^#resource\s+readonly\s+([a-zA-Z_][a-zA-Z_0-9]*(?:\[\])?)\s+([a-zA-Z_][a-zA-Z_0-9]*)\s+:\s+([a-zA-Z_][a-zA-Z_0-9]*)\s*;\s*$"
);

pub struct ShaderPreprocessor {}

impl ShaderPreprocessor {
        pub fn preprocess_glsl_source(
                shader_resources: &ShaderResourceRegistry,
                source_code_path: PathBuf,
        ) -> Result<PreprocessedShaderStage, ShaderLoadError> {
                let source_code = std::fs::read_to_string(&source_code_path)?;

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
                                ShaderDirective::Resource(ShaderResourceDirective {
                                        read_only,
                                        variable_name,
                                        resource_id,
                                        ..
                                }) => {
                                        let resource = shader_resources.get(&resource_id).unwrap();

                                        match &resource.resource_type {
                                                ShaderResourceType::Struct(declaration) => {
                                                        Self::write_children_struct_declarations_rec(
                                                                &mut declared_structs,
                                                                declaration,
                                                                part,
                                                        );
                                                },
                                                ShaderResourceType::DynamicArray { element_type } => {
                                                        Self::write_struct_field_declarations_rec(
                                                                &mut declared_structs,
                                                                element_type,
                                                                part,
                                                        );
                                                },
                                                _ => (),
                                        }

                                        resources.push(ShaderResourceRequirement {
                                                resource_id,
                                                variable_name,
                                                separator_index: parts.len() - 1,
                                                read_only,
                                        });

                                        parts.push(String::new());
                                },
                        }
                }

                let parts = ShaderStageParts(parts);
                let preprocessed_shader = PreprocessedShaderStage {
                        path: source_code_path,
                        parts,
                        resources,
                };

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
                                let resource_directive = Self::parse_resource_shader_directive(line)?;

                                let resource =
                                        shader_resources.get(&resource_directive.resource_id).ok_or_else(|| {
                                                ShaderLoadError::UnknownShaderResourceId(
                                                        resource_directive.resource_id.deref().to_owned(),
                                                )
                                        })?;

                                assert_eq!(
                                        resource.resource_type.resource_type_name(),
                                        resource_directive.type_name
                                );

                                ShaderDirective::Resource(resource_directive)
                        },
                        _ => return Ok(None),
                };

                Ok(Some(directive))
        }

        fn parse_resource_shader_directive(line: &str) -> Result<ShaderResourceDirective, ShaderLoadError> {
                if let Some(captures) = DIRECTIVE_RESOURCE_REGEX.captures(&line) {
                        Ok(ShaderResourceDirective {
                                read_only: false,
                                type_name: captures.get(1).unwrap().as_str().to_string(),
                                variable_name: captures.get(2).unwrap().as_str().to_string(),
                                resource_id: ShaderResourceId::new(captures.get(3).unwrap().as_str()),
                        })
                } else if let Some(captures) = DIRECTIVE_READONLY_RESOURCE_REGEX.captures(&line) {
                        Ok(ShaderResourceDirective {
                                read_only: true,
                                type_name: captures.get(1).unwrap().as_str().to_string(),
                                variable_name: captures.get(2).unwrap().as_str().to_string(),
                                resource_id: ShaderResourceId::new(captures.get(3).unwrap().as_str()),
                        })
                } else {
                        Err(ShaderLoadError::InvalidPreprocessorDirective("resource"))
                }
        }

        fn write_children_struct_declarations_rec(
                declared_structs: &mut HashMap<String, ShaderStructDeclaration>,
                declaration: &ShaderStructDeclaration,
                out: &mut String,
        ) {
                for f in &declaration.fields {
                        Self::write_struct_field_declarations_rec(declared_structs, &f.field_type, out);
                }
        }

        fn write_struct_field_declarations_rec(
                declared_structs: &mut HashMap<String, ShaderStructDeclaration>,
                field_type: &ShaderStructFieldType,
                out: &mut String,
        ) {
                match field_type {
                        ShaderStructFieldType::Struct(declaration) => {
                                Self::write_struct_declarations_rec(declared_structs, declaration, out);
                        },
                        ShaderStructFieldType::Array { element_type, .. } => {
                                Self::write_struct_field_declarations_rec(declared_structs, &element_type, out);
                        },
                        _ => (),
                };
        }

        fn write_struct_declarations_rec(
                declared_structs: &mut HashMap<String, ShaderStructDeclaration>,
                declaration: &ShaderStructDeclaration,
                out: &mut String,
        ) {
                Self::write_children_struct_declarations_rec(declared_structs, declaration, out);

                if let Some(declared) = declared_structs.get(&declaration.type_name) {
                        assert_eq!(declared, declaration); // Check that expected type is correct.
                        return;
                }

                declared_structs.insert(declaration.type_name.clone(), declaration.clone());

                *out += &declaration.glsl_type_declaration();
        }
}

#[derive(Debug)]
pub enum ShaderDirective {
        Resource(ShaderResourceDirective),
}

#[derive(Debug)]
pub struct ShaderResourceDirective {
        read_only: bool,
        type_name: String,
        variable_name: String,
        resource_id: ShaderResourceId,
}

#[derive(Debug, Clone)]
pub struct ShaderResourceRequirement {
        pub resource_id: ShaderResourceId,
        pub variable_name: String,
        pub separator_index: usize,
        pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct PreprocessedShaderStage {
        pub path: PathBuf,
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
