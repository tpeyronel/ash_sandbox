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
        vec4 dir;
        vec4 color;
        vec4 kc_kl_kq;
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

layout (set = 1, binding = 0) uniform texture2D u_texture;
layout (set = 1, binding = 1) uniform sampler u_sampler;
layout (set = 1, binding = 2) uniform MaterialData {
        vec4 ambient_color;
        vec4 diffuse_color;
} u_material;


layout (location = 0) in vec3 i_frag_pos;
layout (location = 1) in vec3 i_normal;

layout (location = 0) out vec4 o_out_color;


void main() {
        o_out_color = vec4(u_lights.point_light.color.rgb, 1.0);

        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));
}
