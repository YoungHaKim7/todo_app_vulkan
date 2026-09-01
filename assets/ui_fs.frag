#version 450

layout(set = 0, binding = 0) uniform sampler s;
layout(set = 0, binding = 1) uniform texture2D tex;

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;
layout(location = 0) out vec4 f_color;

void main() {
    float alpha = texture(sampler2D(tex, s), v_uv).r;
    f_color = vec4(v_color.rgb, v_color.a * alpha);
}
