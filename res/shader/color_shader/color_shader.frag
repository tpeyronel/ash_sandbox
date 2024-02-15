#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource WorldLights u_lights : WORLD_LIGHTS;
#resource MaterialData u_material : MATERIAL_DATA;

layout (location = 0) in vec3 i_frag_pos;
layout (location = 1) in vec3 i_normal;

layout (location = 0) out vec4 o_out_color;


void main() {
        o_out_color = vec4(u_lights.point_light.color.rgb, 1.0);
}
