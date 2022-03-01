#version 450
#extension GL_EXT_debug_printf : enable

struct DirectionalLight {
        vec4 dir;
        vec4 color;
};

struct PointLight {
        vec4 pos;
        vec4 color;
        vec4 kc_kl_kq;
};

struct Spotlight {
        vec4 pos;
        vec4 dir; // xyz=dir w=cutoff angle
        vec4 color;
        vec4 kc_kl_kq_inner;
};

layout (set = 0, binding = 0) uniform WorldMatrices {
        vec4 view_pos;
        mat4 view;
        mat4 proj;
} u_world_matrices;

layout (set = 0, binding = 1) uniform WorldLights {
        DirectionalLight dir_light;
        PointLight point_light;
        Spotlight spotlight;
} u_lights;

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

layout (location = 0) out vec4 o_output;

vec3 calc_diffuse(float diffuse_strength, vec3 normal, vec3 point_light_dir, vec3 point_light_color) {
        float diffuse_angle = max(dot(-point_light_dir, normal), 0.0);

        return diffuse_strength * diffuse_angle * point_light_color;
}

vec3 calc_specular(vec3 camera_rdir, float specular_strength, vec3 normal, vec3 point_light_dir, vec3 point_light_color, float shininess) {
        vec3 halfway = normalize(camera_rdir - point_light_dir);
        float specular_angle = max(dot(halfway, normal), 0.0);
        float specular_coefficient = pow(specular_angle, shininess);

        return specular_strength * specular_coefficient * point_light_color;
}

float calc_attenuation(vec3 kc_kl_kq, float distance) {
        return 1.0 / (kc_kl_kq.x + kc_kl_kq.y * distance + kc_kl_kq.z * distance * distance);
}

vec3 calc_dir_light(
        DirectionalLight dir_light,
        float ambient_strength,
        float specular_strength,
        float diffuse_strength,
        float shininess,
        vec3 camera_rdir,
        vec3 normal,
        vec3 diffuse_texel,
        vec3 specular_texel
) {
        vec3 dir_light_color = u_lights.dir_light.color.rgb;
        vec3 dir_light_dir = normalize(u_lights.dir_light.dir.xyz);

        vec3 ambient = diffuse_texel * ambient_strength * dir_light_color;
        vec3 diffuse = diffuse_texel * calc_diffuse(diffuse_strength, normal, dir_light_dir, dir_light_color);
        vec3 specular = specular_texel * calc_specular(camera_rdir, specular_strength, normal, dir_light_dir, dir_light_color, shininess);

        return ambient + diffuse + specular;
}

vec3 calc_point_light(
        PointLight point_light,
        float ambient_strength,
        float specular_strength,
        float diffuse_strength,
        float shininess,
        vec3 camera_rdir,
        vec3 normal,
        vec3 frag_pos,
        vec3 diffuse_texel,
        vec3 specular_texel
) {
        vec3 point_light_pos = point_light.pos.xyz;
        vec3 point_light_color = point_light.color.rgb;
        vec3 point_light_dir = normalize(frag_pos - point_light_pos);
        float attenuation = calc_attenuation(point_light.kc_kl_kq.xyz, distance(point_light_pos, frag_pos));

        vec3 ambient = attenuation * diffuse_texel * ambient_strength * point_light_color;
        vec3 diffuse = attenuation * diffuse_texel * calc_diffuse(diffuse_strength, normal, point_light_dir, point_light_color);
        vec3 specular = attenuation * specular_texel * calc_specular(camera_rdir, specular_strength, normal, point_light_dir, point_light_color, shininess);

        return ambient + diffuse + specular;
}

vec3 calc_spotlight(Spotlight spotlight, vec3 frag_pos, vec3 diffuse_texel) {
        vec3 spotlight_pos = spotlight.pos.xyz;
        vec3 spotlight_color = spotlight.color.rgb;
        vec3 spotlight_dir_to_frag = normalize(frag_pos - spotlight_pos);
        vec3 spotlight_dir = normalize(spotlight.dir.xyz);
        float spotlight_cutoff_angle = spotlight.dir.w;
        float spotlight_inner_radius_percentage = spotlight.kc_kl_kq_inner.w;
        float spotlight_angle = dot(spotlight_dir_to_frag, spotlight_dir);
        float spotlight_attenuation = calc_attenuation(spotlight.kc_kl_kq_inner.xyz, distance(spotlight_pos, frag_pos));

        // 0.0 = 0°  ---  1.0 = 90°
        float spotlight_normalized_angle = 1.0 - spotlight_angle;
        float spotlight_normalized_cutoff_angle = 1.0 - spotlight_cutoff_angle;

        float spotlight_coefficient = 1.0 - smoothstep(
                spotlight_normalized_cutoff_angle * spotlight_inner_radius_percentage,
                spotlight_normalized_cutoff_angle,
                spotlight_normalized_angle
        );

        vec3 spotlight_diffuse = diffuse_texel * spotlight_color * spotlight_attenuation * spotlight_coefficient;

        return spotlight_diffuse;
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

        vec3 output_color = vec3(0.0);

        output_color += calc_dir_light(
                u_lights.dir_light,
                ambient_strength,
                specular_strength,
                diffuse_strength,
                shininess,
                camera_rdir,
                normal,
                diffuse_texel,
                specular_texel
        );

        output_color += calc_point_light(
                u_lights.point_light,
                ambient_strength,
                specular_strength,
                diffuse_strength,
                shininess,
                camera_rdir,
                normal,
                i_frag_pos,
                diffuse_texel,
                specular_texel
        );

        output_color += calc_spotlight(u_lights.spotlight, i_frag_pos, diffuse_texel);

        float gamma = 2.2;
        output_color = pow(output_color, vec3(1.0 / gamma));

        o_output = vec4(output_color, 1.0);
}
