#version 450

layout (location = 0) in vec4 i_color;

layout (location = 0) out vec4 o_out_color;

void main() {
        o_out_color = i_color;

        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));
}
