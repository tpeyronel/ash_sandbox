#version 450
#extension GL_EXT_debug_printf : enable

layout (set = 0, binding = 0) uniform WorldMatrices {
        vec4 view_pos;
        mat4 view;
        mat4 proj;
        mat4 vp;
} u_world_matrices;

layout (set = 2, binding = 0) uniform ObjectMatrices {
        mat4 model;
        mat4 mvp;
        mat4 normal;
} u_object_matrices;

layout (location = 0) in vec3 i_pos;

layout (location = 0) out vec3 o_uvw;

void main() {
        o_uvw = i_pos;
        o_uvw.x *= -1.0;

        gl_Position = u_object_matrices.mvp * vec4(i_pos, 1.0);
}