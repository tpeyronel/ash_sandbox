#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource ObjectMatrices u_object_matrices : OBJECT_MATRICES;
#resource BillboardData u_billboard : BILLBOARD_DATA;

layout (location = 0) in vec3 i_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;

layout (location = 0) out vec2 o_tex_coord;

void main() {
        vec4 billboard_center = u_object_matrices.model * vec4(0.0, 0.0, 0.0, 1.0);

        vec3 vertex_position = u_billboard.billboard_center.xyz;
        vertex_position = billboard_center.xyz;
        vertex_position += u_billboard.camera_right.xyz * i_pos.x * u_billboard.billboard_scale.x;
        vertex_position += u_billboard.camera_up.xyz * i_pos.y * u_billboard.billboard_scale.y;

        gl_Position = u_world_matrices.proj * u_world_matrices.view * vec4(vertex_position, 1.0);

        o_tex_coord = i_tex_coord;

        // gl_Position = u_world_matrices.proj * u_world_matrices.view * u_object_matrices.model * vec4(i_pos, 1.0);
}