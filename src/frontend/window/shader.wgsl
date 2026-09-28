// Vertex shader bindings

struct VertexOutput {
    @location(0) tex_coord: vec2<f32>,
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );

    let position = positions[index];

    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.tex_coord =
        position * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return out;
}

// Fragment shader bindings

@group(0) @binding(0) var r_tex_color: texture_2d<f32>;
@group(0) @binding(1) var r_tex_sampler: sampler;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let input_size = vec2<f32>(textureDimensions(r_tex_color));
    let tex_coord = uv * input_size;

    let half = vec2<f32>(0.5);
    let one = vec2<f32>(1.0);
    let zero = vec2<f32>(0.0);
    let texels_per_pixel = vec2<f32>(dpdx(tex_coord.x), dpdy(tex_coord.y));
    let tex_coord_fract = fract(tex_coord);
    let tex_coord_x = clamp(tex_coord_fract / texels_per_pixel, zero, half) + clamp((tex_coord_fract - one) / texels_per_pixel + half, zero, half);
    let tex_coord_final = (floor(tex_coord) + tex_coord_x) / input_size;
    return textureSample(r_tex_color, r_tex_sampler, tex_coord_final);
}
