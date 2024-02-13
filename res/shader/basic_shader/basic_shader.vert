#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldLights u_world_lights : WORLD_LIGHTS;
#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource ObjectMatrices u_object_matrices : OBJECT_MATRICES;

// layout (push_constant) uniform Matrices_M_MVP {
//         mat4 model;
//         mat4 mvp;
// } u_mats_m_mvp;

layout (location = 0) in vec3 i_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;
layout (location = 3) in vec4 i_tangent;

layout (location = 0) out vec3 o_frag_pos;
layout (location = 1) out vec3 o_normal;
layout (location = 2) out vec2 o_tex_coord;
layout (location = 3) out mat3 o_tbn;
layout (location = 6) out vec4 o_frag_pos_sun_space;


void main() {
        o_frag_pos = vec3(u_object_matrices.model * vec4(i_pos, 1.0));

        mat3 normal_mat = mat3(u_object_matrices.normal);

        vec3 t = normalize(normal_mat * i_tangent.xyz);
        vec3 n = normalize(normal_mat * i_normal);
        vec3 b = normalize(cross(n, t) * i_tangent.w);
        o_tbn = mat3(t, b, n);

        o_normal = normal_mat * i_normal;
        // o_normal = mat3(transpose(inverse(u_object_matrices.model))) * i_normal;

        o_tex_coord = i_tex_coord;

        o_frag_pos_sun_space = u_world_lights.dir_light.vp * u_object_matrices.model * vec4(i_pos, 1.0);

        gl_Position = u_object_matrices.mvp * vec4(i_pos, 1.0);
}