#version 450
#extension GL_EXT_debug_printf : enable

layout (set = 0, binding = 0) uniform WorldMatrices {
        vec4 view_pos;
        mat4 view;
        mat4 proj;
} u_world_matrices;

layout (set = 2, binding = 0) uniform ObjectMatrices {
        mat4 model;
        mat4 mvp;
        mat4 normal;
} u_object_matrices;

layout (set = 2, binding = 1) uniform BillboardData {
        vec4 center;
        vec4 scale;
        vec4 camera_right;
        vec4 camera_up;
} u_billboard;

layout (location = 0) in vec3 i_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;

layout (location = 0) out vec2 o_tex_coord;

void main() {
        vec4 billboard_center = u_object_matrices.model * vec4(0.0, 0.0, 0.0, 1.0);

        vec3 vertex_position = u_billboard.center.xyz;
        vertex_position = billboard_center.xyz;
        vertex_position += u_billboard.camera_right.xyz * i_pos.x * u_billboard.scale.x;
        vertex_position += u_billboard.camera_up.xyz * i_pos.y * u_billboard.scale.y;

        gl_Position = u_world_matrices.proj * u_world_matrices.view * vec4(vertex_position, 1.0);

        o_tex_coord = i_tex_coord;

        // gl_Position = u_world_matrices.proj * u_world_matrices.view * u_object_matrices.model * vec4(i_pos, 1.0);
}