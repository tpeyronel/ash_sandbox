#version 450
#extension GL_EXT_debug_printf : enable

layout (set = 0, binding = 0) uniform WorldMatrices {
        vec4 view_pos;
        mat4 view;
        mat4 proj;
} u_world_matrices;

layout (set = 0, binding = 1) uniform WorldDirectionalLight {
        vec4 direction;
        vec4 color;
} u_dir_light;

layout (set = 0, binding = 2) uniform WorldLight {
        vec4 pos;
        vec4 color;
} u_light;

layout (set = 1, binding = 0) uniform MaterialData {
        vec4 ambient_color;
        vec4 diffuse_color;
        vec4 specular_color;
        vec2 shininess_and_ambient_strength;
        vec2 specular_strength_and_diffuse_strength;
} u_material;
layout (set = 1, binding = 1) uniform texture2D u_diffuse_map;
layout (set = 1, binding = 2) uniform texture2D u_specular_map;
layout (set = 1, binding = 3) uniform sampler u_sampler;


layout (location = 0) in vec3 i_frag_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;

layout (location = 0) out vec4 o_out_color;

vec3 calculate_diffuse(float diffuse_strength, vec3 normal, vec3 light_dir, vec3 light_color) {
        float diffuse_angle = max(dot(-light_dir, normal), 0.0);

        return diffuse_strength * diffuse_angle * light_color;
}

vec3 calculate_specular(vec3 camera_rdir, float specular_strength, vec3 normal, vec3 light_dir, vec3 light_color, float shininess) {
        vec3 halfway = normalize(camera_rdir - light_dir);
        float specular_angle = max(dot(halfway, normal), 0.0);
        float specular_coefficient = pow(specular_angle, shininess);

        return specular_strength * specular_coefficient * light_color;
}

void main() {
        float shininess = u_material.shininess_and_ambient_strength.x;
        float ambient_strength = u_material.shininess_and_ambient_strength.y;
        float specular_strength = u_material.specular_strength_and_diffuse_strength.x;
        float diffuse_strength = u_material.specular_strength_and_diffuse_strength.y;

        vec3 diffuse_texel = texture(sampler2D(u_diffuse_map, u_sampler), i_tex_coord).rgb;
        vec3 specular_texel = texture(sampler2D(u_specular_map, u_sampler), i_tex_coord).rgb;


        vec3 camera_rdir = normalize(u_world_matrices.view_pos.xyz - i_frag_pos);
        vec3 normal = normalize(i_normal);
        vec3 light_dir = normalize(i_frag_pos - u_light.pos.xyz);
        vec3 dir_light_dir = normalize(u_dir_light.direction.xyz);

        vec3 ambient = ambient_strength * u_dir_light.color.rgb * diffuse_texel;

        vec3 light_diffuse = calculate_diffuse(diffuse_strength, normal, light_dir, u_light.color.rgb);
        vec3 dir_light_diffuse = calculate_diffuse(diffuse_strength, normal, dir_light_dir, u_dir_light.color.rgb);
        vec3 diffuse = (light_diffuse + dir_light_diffuse) * diffuse_texel;

        vec3 light_specular = calculate_specular(camera_rdir, specular_strength, normal, light_dir, u_light.color.rgb, shininess);
        vec3 dir_light_specular = calculate_specular(camera_rdir, specular_strength, normal, dir_light_dir, u_dir_light.color.rgb, shininess);
        vec3 specular = (light_specular + dir_light_specular) * specular_texel;

        o_out_color = vec4(ambient + specular + diffuse, 1.0);


        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));
}
