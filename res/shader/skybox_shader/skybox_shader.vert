#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource ObjectMatrices u_object_matrices : OBJECT_MATRICES;

layout (location = 0) in vec3 i_pos;

layout (location = 0) out vec3 o_uvw;

void main() {
        o_uvw = i_pos;
        o_uvw.x *= -1.0;

        gl_Position = u_object_matrices.mvp * vec4(i_pos, 1.0);
}