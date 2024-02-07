#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldLights u_world_lights : WORLD_LIGHTS;
#resource ObjectMatrices u_object_matrices : OBJECT_MATRICES;

layout (location = 0) in vec3 i_pos;

layout (location = 0) out vec3 o_frag_pos;

layout (push_constant) uniform constants {
        int face_index;
} u_push_constants;

void main() {
        vec4 frag_pos = u_object_matrices.model * vec4(i_pos, 1.0);

        o_frag_pos = frag_pos.xyz;

        mat4 vp_mat = u_world_lights.point_light.vp_mats[u_push_constants.face_index];

        gl_Position = vp_mat * frag_pos;
}