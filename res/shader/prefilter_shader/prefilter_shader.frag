#version 450
#extension GL_EXT_debug_printf : enable
#define PI 3.14159265358979323846264338327950288

#resource samplerCube u_environment_map : ENVIRONMENT_MAP;
#resource PrefilterParams u_prefilter_params : PREFILTER_PARAMS;

layout (location = 0) in vec3 i_pos;

layout (location = 0) out vec4 o_output;

// TODO: dedup (pbr_shader.frag)
float distribution_ggx(float n_dot_h, float roughness) {
        float alpha = roughness * roughness;
        float alpha_squared = alpha * alpha;
        float n_dot_h_squared = n_dot_h * n_dot_h;

        float denominator = n_dot_h_squared * (alpha_squared - 1.0) + 1.0;
        denominator = PI * denominator * denominator;

        return alpha_squared / denominator;
}

float compute_mip_level(float roughness, float n_dot_h, float h_dot_v, float env_map_size, uint sample_count) {
        float d = distribution_ggx(n_dot_h, roughness);
        float pdf = (d * n_dot_h / (4.0 * h_dot_v)) + 0.0001;

        float sa_texel = 4.0 * PI / (6.0 * env_map_size * env_map_size);
        float sa_sample = 1.0 / (float(sample_count) * pdf + 0.0001);

        float mip_level = (roughness == 0.0) ? 0.0 : 0.5 * log2(sa_sample / sa_texel);
        return mip_level;
}

float radical_inverse_vdc(uint bits) {
        bits = (bits << 16u) | (bits >> 16u);
        bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
        bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
        bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
        bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
        return float(bits) * 2.3283064365386963e-10; // / 0x100000000
}

vec2 hammersley(uint i, uint n) {
        return vec2(float(i) / float(n), radical_inverse_vdc(i));
}

vec3 importance_sample_ggx(vec2 x_i, vec3 normal, float roughness) {
        float a = roughness * roughness;

        float phi = 2.0 * PI * x_i.x;
        float cos_theta = sqrt((1.0 - x_i.y) / (1.0 + (a * a - 1.0) * x_i.y));
        float sin_theta = sqrt(1.0 - cos_theta * cos_theta);

        // from spherical coordinates to cartesian coordinates
        vec3 H;
        H.x = cos(phi) * sin_theta;
        H.y = sin(phi) * sin_theta;
        H.z = cos_theta;

        // from tangent-space vector to world-space sample vector
        vec3 up        = abs(normal.z) < 0.999 ? vec3(0.0, 0.0, 1.0) : vec3(1.0, 0.0, 0.0);
        vec3 tangent   = normalize(cross(up, normal));
        vec3 bitangent = cross(normal, tangent);

        vec3 sample_vec = tangent * H.x + bitangent * H.y + normal * H.z;
        return normalize(sample_vec);
}

#define SAMPLE_COUNT 1024

void main() {
        float roughness = u_prefilter_params.roughness_and_env_map_size.x;
        float env_map_size = u_prefilter_params.roughness_and_env_map_size.y;

        vec3 normal = normalize(i_pos);
        vec3 r = normal;
        vec3 v = r;

        float total_weight = 0.0;
        vec3 prefiltered_color = vec3(0.0);

        for (uint i = 0u; i < SAMPLE_COUNT; ++i) {
                vec2 x_i = hammersley(i, SAMPLE_COUNT);
                vec3 h = importance_sample_ggx(x_i, normal, roughness);
                vec3 l = normalize(2.0 * dot(v, h) * h - v);

                float n_dot_l = max(dot(normal, l), 0.0);
                float n_dot_h = max(dot(normal, h), 0.0);
                float h_dot_v = max(dot(h, v), 0.0);

                float mip_level = compute_mip_level(roughness, n_dot_h, h_dot_v, env_map_size, SAMPLE_COUNT);

                prefiltered_color += min(textureLod(u_environment_map, l, mip_level).rgb, 50.0) * n_dot_l;
                total_weight += n_dot_l;
        }

        prefiltered_color /= total_weight;

        o_output = vec4(prefiltered_color, 1.0);
}
