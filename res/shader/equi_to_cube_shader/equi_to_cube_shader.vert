#version 450
#extension GL_EXT_debug_printf : enable

layout (location = 0) out vec3 o_pos;

layout (push_constant) uniform constants {
        mat4 rotation;
} u_push_constants;

const vec2 positions[6] = vec2[6](
        vec2(-1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, -1.0),
        vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0)
);

void main() {
        gl_Position = vec4(positions[gl_VertexIndex], 1.0, 1.0);
        vec3 pos = vec3(gl_Position.xy, 1.0);
        o_pos = mat3(u_push_constants.rotation) * pos;
}