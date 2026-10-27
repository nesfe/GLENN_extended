struct Uniforms {
    camera: mat4x4<f32>,
    clock: vec4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
struct Input {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    // Material: 0 = lit surface, 1 = tracer, 2 = unlit reference.
    @location(3) data: vec4<f32>,
};
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) data: vec4<f32>,
};
@vertex fn vs_main(input: Input) -> Output {
    var out: Output;
    out.position = uniforms.camera * vec4(input.position, 1.0);
    out.normal = input.normal;
    out.color = input.color;
    out.data = input.data;
    return out;
}
@fragment fn fs_main(input: Output) -> @location(0) vec4<f32> {
    var color = input.color;
    if input.data.x < 0.5 {
        let normal = normalize(input.normal);
        let light = normalize(vec3(-0.5, 1.0, 0.4));
        let diffuse = max(dot(normal, light), 0.0);
        let rim = pow(1.0 - abs(dot(normal, uniforms.clock.yzw)), 3.0);
        let halfway = normalize(light + uniforms.clock.yzw);
        let specular = pow(max(dot(normal, halfway), 0.0), 64.0);
        color = vec4(color.rgb * (0.48 + 0.52 * diffuse) + vec3(0.08) * rim + vec3(0.24) * specular, color.a);
    } else if input.data.x < 1.5 {
        // Travel time is integrated by the CPU solver. Equal phase intervals
        // therefore move faster where the calculated local speed is higher.
        let phase = fract((input.data.y - uniforms.clock.x) / 1.2);
        let pulse = pow(max(0.0, 1.0 - abs(phase - 0.5) * 12.0), 3.0);
        color = vec4(mix(color.rgb, vec3(0.9, 0.99, 1.0), pulse * 0.8), color.a * (0.38 + 0.62 * pulse));
    }
    return color;
}

struct LineInput {
    @location(0) start: vec4<f32>,
    @location(1) end: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) data: vec4<f32>,
};
@vertex fn vs_line(input: LineInput, @builtin(vertex_index) index: u32) -> Output {
    let corners = array<vec2<f32>,6>(vec2(0.0,-1.0),vec2(1.0,-1.0),vec2(1.0,1.0),vec2(0.0,-1.0),vec2(1.0,1.0),vec2(0.0,1.0));
    let corner=corners[index];
    let a=uniforms.camera*vec4(input.start.xyz,1.0);
    let b=uniforms.camera*vec4(input.end.xyz,1.0);
    let delta=(b.xy/b.w-a.xy/a.w)*uniforms.viewport.xy;
    let normal=vec2(-delta.y,delta.x)/max(length(delta),1e-8);
    let scale=length(vec3(uniforms.camera[0].x,uniforms.camera[1].x,uniforms.camera[2].x));
    let width=clamp(input.data.x*scale*uniforms.viewport.x,0.8,4.0);
    var position=mix(a,b,corner.x);
    position=vec4(position.xy+normal*corner.y*width/uniforms.viewport.xy*position.w,position.zw);
    var out:Output;
    out.position=position; out.normal=vec3(0.0,1.0,0.0);out.color=input.color;
    out.data=vec4(input.data.y,mix(input.start.w,input.end.w,corner.x),0.0,0.0);
    return out;
}
