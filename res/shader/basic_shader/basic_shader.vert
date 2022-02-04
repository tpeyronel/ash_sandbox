#version 450
#extension GL_EXT_debug_printf : enable

layout (set = 0, binding = 0) uniform WorldMatrices {
        mat4 view;
        mat4 proj;
} u_world_matrices;

layout (set = 2, binding = 0) uniform ObjectMatrices {
        mat4 model;
        mat4 mvp;
        mat4 normal;
} u_object_matrices;


layout (push_constant) uniform Matrices_M_MVP {
        mat4 model;
        mat4 mvp;
} u_mats_m_mvp;


layout (location = 0) in vec3 i_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;

layout (location = 0) out vec3 o_frag_pos;
layout (location = 1) out vec3 o_normal;
layout (location = 2) out vec2 o_tex_coord;


void main() {
        o_frag_pos = vec3(u_world_matrices.view * u_object_matrices.model * vec4(i_pos, 1.0));
        // o_normal = mat3(u_object_matrices.normal) * i_normal;
        o_normal = mat3(transpose(inverse(u_world_matrices.view * u_object_matrices.model))) * i_normal;

        o_tex_coord = i_tex_coord;

        gl_Position = u_world_matrices.proj * u_world_matrices.view * u_object_matrices.model * vec4(i_pos, 1.0);
}