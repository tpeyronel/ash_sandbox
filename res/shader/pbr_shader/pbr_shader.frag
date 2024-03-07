#version 450
#extension GL_EXT_debug_printf : enable

#define PI 3.14159265358979323846264338327950288

#resource samplerCube u_cube_shadow_map : CUBE_SHADOW_MAP;
#resource samplerCube u_irradiance_map : IRRADIANCE_MAP;
#resource sampler2D u_shadow_map : SHADOW_MAP;
#resource ShaderSettings u_settings : SHADER_SETTINGS;
#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource WorldLights u_lights : WORLD_LIGHTS;
#resource MaterialData u_material : MATERIAL_DATA;
#resource sampler2D u_base_color_map : MATERIAL_BASE_COLOR_TEXTURE;
#resource sampler2D u_metallic_roughness_map : MATERIAL_METALLIC_ROUGHNESS_TEXTURE;
#resource sampler2D u_normal_map : MATERIAL_NORMAL_TEXTURE;

layout (location = 0) in vec3 i_frag_world_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;
layout (location = 3) in mat3 i_tbn;
layout (location = 6) in vec4 i_frag_pos_sun_space;

layout (location = 0) out vec4 o_output;

// vec3 calc_diffuse(float diffuse_strength, vec3 normal, vec3 point_light_dir, vec3 point_light_color) {
//         float diffuse_angle = max(dot(-point_light_dir, normal), 0.0);

//         return diffuse_strength * diffuse_angle * point_light_color;
// }

// vec3 calc_specular(vec3 camera_rdir, float specular_strength, vec3 normal, vec3 point_light_dir, vec3 point_light_color, float shininess) {
//         vec3 halfway = normalize(camera_rdir - point_light_dir);
//         float specular_angle = max(dot(halfway, normal), 0.0);
//         float specular_coefficient = pow(specular_angle, shininess);

//         return specular_strength * specular_coefficient * point_light_color;
// }

// float calc_attenuation(vec3 kc_kl_kq, float distance) {
//         return 1.0 / (kc_kl_kq.x + kc_kl_kq.y * distance + kc_kl_kq.z * distance * distance);
// }

// float calc_shadow() {
//         // Note: we only need to normalize xy and not z, because depth is already in [0, 1] range

//         vec3 proj_coords = i_frag_pos_sun_space.xyz;
//         vec2 uv = i_frag_pos_sun_space.xy * 0.5 + 0.5; // from [-1, 1] to [0, 1]
//         uv.y = 1.0 - uv.y;

//         float stored_depth = texture(u_shadow_map, uv).r;
//         float current_depth = min(proj_coords.z, 1.0);
//         float depth_bias = 0.00001;

//         float shadow = 0.0;
//         vec2 texel_size_in_uv = 1.0 / textureSize(u_shadow_map, 0);
//         for(int x = -1; x <= 1; ++x) {
//                 for(int y = -1; y <= 1; ++y) {
//                         float pcf_depth = texture(u_shadow_map, uv + vec2(x, y) * texel_size_in_uv).r;
//                         shadow += float(current_depth <= pcf_depth + depth_bias);
//                 }
//         }
//         shadow /= 9.0;

//         return shadow;
// }

// vec3 calc_dir_light(
//         WorldDirectionalLight dir_light,
//         float ambient_strength,
//         float specular_strength,
//         float diffuse_strength,
//         float shininess,
//         vec3 camera_rdir,
//         vec3 normal,
//         vec3 diffuse_texel,
//         vec3 specular_texel
// ) {
//         vec3 dir_light_color = u_lights.dir_light.color_and_intensity.rgb * u_lights.dir_light.color_and_intensity.w;
//         vec3 dir_light_dir = normalize(u_lights.dir_light.direction.xyz);

//         vec3 ambient = diffuse_texel * ambient_strength * dir_light_color;
//         vec3 diffuse = diffuse_texel * calc_diffuse(diffuse_strength, normal, dir_light_dir, dir_light_color);
//         vec3 specular = specular_texel * calc_specular(camera_rdir, specular_strength, normal, dir_light_dir, dir_light_color, shininess);

//         float shadow = calc_shadow();

//         return ambient + (shadow * (diffuse + specular));
// }

// float calc_point_shadow() {
//         vec3 point_light_to_frag = i_frag_pos - u_lights.point_light.pos.xyz;

//         float current_depth = length(point_light_to_frag);
//         float stored_depth = texture(u_cube_shadow_map, point_light_to_frag).r;

//         // float direct_measure = dot(normalize(i_normal), normalize(-point_light_to_frag));
//         // float depth_bias = 0.01 / max(direct_measure, 0.001) - 0.01 + 0.0001;
//         float depth_bias = 0.005;

//         return float(current_depth <= stored_depth + depth_bias);
// }

// vec3 calc_point_light(
//         WorldPointLight point_light,
//         float ambient_strength,
//         float specular_strength,
//         float diffuse_strength,
//         float shininess,
//         vec3 camera_rdir,
//         vec3 normal,
//         vec3 frag_pos,
//         vec3 diffuse_texel,
//         vec3 specular_texel
// ) {
//         vec3 point_light_pos = point_light.pos.xyz;
//         vec3 point_light_color = point_light.color.rgb;
//         vec3 point_light_dir = normalize(frag_pos - point_light_pos);
//         float attenuation = calc_attenuation(point_light.kc_kl_kq.xyz, distance(point_light_pos, frag_pos));

//         vec3 ambient = attenuation * diffuse_texel * ambient_strength * point_light_color;
//         vec3 diffuse = attenuation * diffuse_texel * calc_diffuse(diffuse_strength, normal, point_light_dir, point_light_color);
//         vec3 specular = attenuation * specular_texel * calc_specular(camera_rdir, specular_strength, normal, point_light_dir, point_light_color, shininess);

//         float shadow = calc_point_shadow();

//         return ambient + (shadow * (diffuse + specular));
// }

// vec3 calc_spotlight(
//         WorldSpotlight spotlight,
//         float specular_strength,
//         float diffuse_strength,
//         float shininess,
//         vec3 camera_rdir,
//         vec3 normal,
//         vec3 frag_pos,
//         vec3 diffuse_texel,
//         vec3 specular_texel
// ) {
//         vec3 spotlight_pos = spotlight.pos.xyz;
//         vec3 spotlight_color = spotlight.color.rgb;
//         vec3 spotlight_dir_to_frag = normalize(frag_pos - spotlight_pos);
//         vec3 spotlight_dir = normalize(spotlight.dir.xyz);
//         float spotlight_cutoff_angle = spotlight.dir.w;
//         float spotlight_inner_radius_percentage = spotlight.kc_kl_kq_inner.w;
//         float spotlight_angle = dot(spotlight_dir_to_frag, spotlight_dir);
//         float attenuation = calc_attenuation(spotlight.kc_kl_kq_inner.xyz, distance(spotlight_pos, frag_pos));

//         // 0.0 = 0°  ---  1.0 = 90°
//         float spotlight_normalized_angle = 1.0 - spotlight_angle;
//         float spotlight_normalized_cutoff_angle = 1.0 - spotlight_cutoff_angle;

//         float spotlight_coefficient = 1.0 - smoothstep(
//                 spotlight_normalized_cutoff_angle * spotlight_inner_radius_percentage,
//                 spotlight_normalized_cutoff_angle,
//                 spotlight_normalized_angle
//         );

//         vec3 diffuse =
//                 spotlight_coefficient
//                 * attenuation
//                 * diffuse_texel
//                 * calc_diffuse(diffuse_strength, normal, spotlight_dir_to_frag, spotlight_color);

//         vec3 specular =
//                 spotlight_coefficient
//                 * attenuation
//                 * specular_texel
//                 * calc_specular(camera_rdir, specular_strength, normal, spotlight_dir_to_frag, spotlight_color, shininess);

//         return diffuse + specular;
// }

vec3 fetch_normal() {
        if (u_settings.alt_normals.x == 0) {
                vec3 normal = texture(u_normal_map, i_tex_coord).xyz * 2.0 - 1.0;
                return normalize(i_tbn * normal);
        } else {
                return normalize(i_normal);
        }
}

vec3 fresnel_schlick(float cos_theta, vec3 f_0) {
        return f_0 + ((1.0 - f_0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0));
}

vec3 fresnel_schlick_roughness(float cos_theta, vec3 f_0, float roughness) {
        return f_0 + (max(vec3(1.0 - roughness), f_0) - f_0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

float distribution_ggx(vec3 normal, vec3 halfway, float roughness) {
        float alpha = roughness * roughness;
        float alpha_squared = alpha * alpha;
        float n_dot_h = max(dot(normal, halfway), 0.0);
        float n_dot_h_squared = n_dot_h * n_dot_h;

        float denominator = n_dot_h_squared * (alpha_squared - 1.0) + 1.0;
        denominator = PI * denominator * denominator;

        return alpha_squared / denominator;
}

float geometry_schlick_ggx(float n_dot_v, float roughness) {
        float r = (roughness + 1.0);
        float k = (r * r) * (1.0 / 8.0);

        float denominator = n_dot_v * (1.0 - k) + k;

        return n_dot_v / denominator;
}

float geometry_smith(float n_dot_v, float n_dot_l, float roughness) {
        float ggx_1 = geometry_schlick_ggx(n_dot_v, roughness);
        float ggx_2 = geometry_schlick_ggx(n_dot_l, roughness);

        return ggx_1 * ggx_2;
}

void main() {
        vec3 albedo = texture(u_base_color_map, i_tex_coord).rgb;
        vec2 metallic_roughness = texture(u_metallic_roughness_map, i_tex_coord).zy;
        float metallic = metallic_roughness.x;
        float roughness = metallic_roughness.y;

        // vec3 albedo = vec3(1.0, 0.0, 0.0);
        // float metallic = 0.2;
        // float roughness = 0.1;

        // calculate reflectance at normal incidence; if dia-electric (like plastic) use f_0
        // of 0.04 and if it's a metal, use the albedo color as f_0 (metallic workflow)
        vec3 f_0 = mix(vec3(0.04), albedo, metallic);

        // aka n
        vec3 normal = fetch_normal();

        // aka v
        vec3 frag_to_view = normalize(u_world_matrices.view_pos.xyz - i_frag_world_pos);

        vec3 total_radiance = vec3(0.0);

        // point light
        {
                vec3 frag_to_light_raw = u_lights.point_light.pos.xyz - i_frag_world_pos;
                // aka l
                vec3 frag_to_light = normalize(u_lights.point_light.pos.xyz - i_frag_world_pos);

                // aka h
                vec3 halfway = normalize(frag_to_view + frag_to_light);

                float distance_squared = dot(frag_to_light_raw, frag_to_light_raw);
                float attenuation = 1.0 / distance_squared;

                vec3 radiance = u_lights.point_light.color.rgb * attenuation;

                float ndf = distribution_ggx(normal, halfway, roughness);

                float n_dot_v = max(dot(normal, frag_to_view), 0.0);
                float n_dot_l = max(dot(normal, frag_to_light), 0.0);
                float geometry = geometry_smith(n_dot_v, n_dot_l, roughness);

                float cos_theta = dot(halfway, frag_to_view);
                vec3 fresnel = fresnel_schlick(cos_theta, f_0);

                vec3 numerator = ndf * geometry * fresnel;
                float denominator = max(4.0 * n_dot_v * n_dot_l, 0.0001);
                vec3 specular = numerator / denominator;

                vec3 k_s = fresnel;
                vec3 k_d = (vec3(1.0) - k_s) * (1.0 - metallic);

                total_radiance += (k_d * albedo * (1.0 / PI) + specular) * radiance * n_dot_l;
        }

        // ambient lighting
        {
                // vec3 f_0 = vec3(0.04);
                // float roughness = 0.5;
                // float metallic = 0.0;
                // vec3 albedo = vec3(1.0);

                float cos_theta = dot(normal, frag_to_view);
                vec3 fresnel = fresnel_schlick_roughness(cos_theta, f_0, roughness);
                vec3 k_s = fresnel;
                vec3 k_d = (vec3(1.0) - k_s) * (1.0 - metallic);

                vec3 ambient_irradiance = texture(u_irradiance_map, vec3(normal.x, normal.y, -normal.z)).rgb;
                vec3 ambient_diffuse = ambient_irradiance * albedo;
                vec3 ambient = k_d * ambient_diffuse /* * ao */;

                total_radiance += ambient;
        }

        vec3 output_color = vec3(0.0);

        // output_color += calc_dir_light(
        //         u_lights.dir_light,
        //         ambient_strength,
        //         specular_strength,
        //         diffuse_strength,
        //         shininess,
        //         camera_rdir,
        //         normal,
        //         diffuse_texel,
        //         specular_texel
        // );

        // output_color += calc_point_light(
        //         u_lights.point_light,
        //         ambient_strength,
        //         specular_strength,
        //         diffuse_strength,
        //         shininess,
        //         camera_rdir,
        //         normal,
        //         i_frag_pos,
        //         diffuse_texel,
        //         specular_texel
        // );

        // output_color += calc_spotlight(
        //         u_lights.spotlight,
        //         specular_strength,
        //         diffuse_strength,
        //         shininess,
        //         camera_rdir,
        //         normal,
        //         i_frag_pos,
        //         diffuse_texel,
        //         specular_texel
        // );

        // o_output = vec4(output_color, 1.0);
        o_output = vec4(total_radiance, 1.0);
}
