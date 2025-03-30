struct Hermite {
    float3 pos;
    float3 normal;
}

struct Camera {
    float width;
    float height;
    float3x3 frame;
    float3x3 frame_inv;
    float3 centre;
    float yfov;
}

struct TR3
{
    float3 q;
    float3 v;
}

struct SurfaceParams: IDifferentiable {
    float support;
    int point_count;
    StructuredBuffer<Hermite> point_data;
}

func wendland(no_diff h: float, x: float) -> float {
    if (x > h) {
        return 0;
    } else {
        return powr(1 - x / h, 4) * (4 * x / h + 1);
    }
}

// TODO: actual metric
func metric(pos: float3)->float3x3 {
    return float3x3(float3(1.0, 0.0, 0.0), float3(0.0, 1.0, 0.0), float3(0.0, 0.0, 1.0));
}

// 0 <= lo <= hi < hs.getCount()
func local_centroid(no_diff sp: SurfaceParams, x: float3)->Hermite {
    var weighted_normal_sum : float3 = float3(0);
    var weighted_pos_sum : float3 = float3(0);
    var weight_sum : float = 0;
    for (int i = 0; i < sp.point_count; ++i) {
        let w = wendland(sp.support, length(x - sp.point_data[i].pos));
        weighted_normal_sum += sp.point_data[i].normal * w;
        weighted_pos_sum += sp.point_data[i].pos * w;
        weight_sum += w;
    }
    return Hermite(weighted_pos_sum / weight_sum, weighted_normal_sum / weight_sum);
}

[Differentiable]
func surface_energy(no_diff sp: SurfaceParams, x: float3)-> float {
    let lc = local_centroid(sp, x);
    return dot(lc.normal, x - lc.pos);
}

// Main idea: we're using Newton's method for improving lambda.
// By taking grad at the new base point that was nudged
// along a line closer to the zero surface, we hopefully get an improved
// direction too.
// Derivation proceeds from thinking about f(x-lambda delta) as a function of lambda.
func project_onto_surface(no_diff sp: SurfaceParams, x: float3)->float3 {
    let KSTEPS = 10;
    var lambda : float = 0;
    var yPair : DifferentialPair<float3> = diffPair(x, 0);
    bwd_diff(surface_energy)(sp, yPair, 1.0);
    // Invariant: yPair.v approx to basepoint, yPair.d approx to grad f at basepoint
    // x - lambda * yPair.d approx basepoint
    for (int k = 0; k < KSTEPS; ++k) {
        yPair = diffPair(x - lambda * yPair.d, yPair.d);
        bwd_diff(surface_energy)(sp, yPair, 1.0);
        lambda += surface_energy(sp, yPair.v)/dot(yPair.d, yPair.d);
    }
    return yPair.v;
}

func base_and_delta(no_diff sp: SurfaceParams, x: float3)->TR3 {
    let base = project_onto_surface(sp, x);
    return TR3(base, x - base);
}

func transition_point(no_diff sp: SurfaceParams, no_diff outer_length: float, x: float3)->float3 {
    let bd = base_and_delta(sp, x);
    let new_delta = (outer_length - length(bd.v)) * normalize(bd.v);
    return bd.q + new_delta;
}

func fragpos_to_ray(camera: Camera, pos: float2)->TR3 {
    let ray_coords = normalize(float3(
        (pos.x / camera.width - 0.5) * (camera.width / camera.height),
        pos.y / camera.height - 0.5,
        0.5 * rcp(tan(camera.yfov / 2.0))
    ));
    let ray = mul(camera.frame, ray_coords);
    return TR3(camera.centre, ray);
}

func global_point_to_camera(c: Camera, g: float3)->float3 {
    return mul(c.frame_inv, g - c.centre);
}

func camera_point_to_global(c: Camera, l: float3)->float3 {
    return mul(c.frame, l) + c.centre;
}

func global_vec_to_camera(c: Camera, gv: float3)->float3 {
    return mul(c.frame_inv, gv);
}

func camera_vec_to_global(c: Camera, lv: float3)->float3 {
    return mul(c.frame, lv);
}

func direct_render_hermite(c: Camera, h: Hermite, pos: float2)->float4 {
    let a = h.pos;
    let b = a + h.normal;
    let la = global_point_to_camera(c, a);
    let lb = global_point_to_camera(c, b);
    // Not dealing with the camera plane intersection case right now
    if (la.z <= 0 || lb.z <= 0) {
        return float4(0);
    }
    let sa = la.xy / la.z;
    let sb = lb.xy / lb.z;
    let central_pos = float2(pos.x - c.width / 2, pos.y - c.height / 2);
    let inverse_focal_length = (2 * tan(c.yfov / 2)) / c.height; // also pixel width
    let rescaled_pos = central_pos * inverse_focal_length;
    let lambda = dot(sb - sa, sb - rescaled_pos) / dot(sb - sa, sb - sa);
    if (lambda < 0 || lambda > 1) {
        return float4(0);
    }
    let nearest_to_pos_on_segment = lerp(sb, sa, lambda); // lambda * sa + (1-lambda) * sb
    let absed_diff = abs(nearest_to_pos_on_segment - rescaled_pos);
    if (max(absed_diff.x, absed_diff.y) > inverse_focal_length / 2) {
        return float4(0);
    }
    return float4(lambda, 1 - lambda, 0, 1);
}

// DATA
ConstantBuffer<Camera> camera;
ConstantBuffer<SurfaceParams> surface;
ConstantBuffer<TextureCube> background;
ConstantBuffer<SamplerState> background_sampler;

// ENTRY POINTS
[shader("vertex")]
float4 vertex(uint ix: SV_VertexID)
    : SV_Position
{
    float4 vertices[3] = {
        float4(-1.0, -1.0, 0.0, 1.0),
        float4(3.0, -1.0, 0.0, 1.0),
        float4(-1.0, 3.0, 0.0, 1.0)
    };
    return vertices[ix];
}

[shader("fragment")]
float4 fragment(float4 in: SV_Position)
    : SV_Target
{
    for (int i = 0; i < surface.point_count; ++i) {
        let h = surface.point_data[i];
        let color = direct_render_hermite(camera, h, in.xy);
        if (color.w > 0.5) {
            return color;
        }
    }
    let ray = fragpos_to_ray(camera, in.xy);
    return background.Sample(background_sampler, ray.v);
}