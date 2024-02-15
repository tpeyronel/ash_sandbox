#version 450
#extension GL_EXT_debug_printf : enable

layout (location = 0) out vec2 o_tex_coords;

void main() {
        // https://www.saschawillems.de/blog/2016/08/13/vulkan-tutorial-on-rendering-a-fullscreen-quad-without-buffers/
        o_tex_coords = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);

        gl_Position = vec4(o_tex_coords * 2.0 + -1.0, 0.0, 1.0);
}