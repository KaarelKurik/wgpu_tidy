struct Hermite : IDifferentiable
{
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

struct TR3 : IDifferentiable
{
    float3 q;
    float3 v;
}

struct SituatedPoint : IDifferentiable
{
    int region;
    float3 q;
}

struct SituatedTR3 : IDifferentiable
{
    int region;
    float3 q;
    float3 v;
}

struct SurfaceParams: IDifferentiable {
    float support;
    int point_count;
    StructuredBuffer<Hermite> point_data;
}

// Only allows x >= 0.
[BackwardDifferentiable]
func wendland(no_diff h: float, x: float) -> float {
    if (x > h) {
        return 0;
    } else {
        var t = 1 - x / h;
        t = t * t;
        t = t * t;
        return t * (4 * x / h + 1);
    }
}

[ForwardDifferentiable]
func smootherstep(x: float)->float {
    if (x < 0) {
        return 0;
    }
    if (x > 1) {
        return 1;
    }
    return x * x * x * (10 + x * (-15 + x * 6));
}

func transition_fiber(no_diff outer_length: float, old_bd: SituatedTR3)->SituatedTR3 {
    return SituatedTR3(1 - old_bd.region, old_bd.q, (outer_length - length(old_bd.v)) * normalize(old_bd.v));
}

// Suppose we have metric g on side A and metric h on side B. Coherence demands that
// the pullback of h by the A->B transition must be g. We're in a situation where g=h,
// so we're looking at pullback(g)=g. Since we define g = lambda*r + pullback(lambda)*pullback(r),
// and pullback composed with itself is identity (bc our transition is own inverse), we
// get pullback(g) = g with no issues.
[ForwardDifferentiable]
func metric(no_diff sp: SurfaceParams, no_diff outer_length: float, x: SituatedPoint)->float3x3 {
    let bd = base_and_delta(sp, x.q);
    let other_bd = TR3(bd.q, (outer_length - length(bd.v)) * normalize(bd.v));
    let here_raw_fiber_param = length(bd.v) / outer_length;
    let there_raw_fiber_param = length(other_bd.v) / outer_length;
    let here_fiber_param = smootherstep(here_raw_fiber_param);
    let there_fiber_param = smootherstep(there_raw_fiber_param);
    let here_metric = float3x3(float3(1, 0, 0), float3(0, 1, 0), float3(0, 0, 1));
    let jacobian = transition_jacobian(sp, outer_length, x.q);
    let there_metric_pullback = transpose(jacobian) * jacobian; // Would have metric in the middle if it weren't the identity
    return here_fiber_param * here_metric + there_fiber_param * there_metric_pullback; 
}

// 0 <= lo <= hi < hs.getCount()
[BackwardDerivative(manual_bwd_diff_of_local_centroid)]
func local_centroid(no_diff sp: SurfaceParams, x: float3)->Hermite {
    var weighted_normal_sum : float3 = float3(0);
    var weighted_pos_sum : float3 = float3(0);
    var weight_sum : float = 0;
    // Probably this is very very bad
    // [MaxIters(100)]
    for (int i = 0; i < sp.point_count; ++i) {
        let w = wendland(sp.support, length(x - sp.point_data[i].pos));
        weighted_normal_sum += sp.point_data[i].normal * w;
        weighted_pos_sum += sp.point_data[i].pos * w;
        weight_sum += w;
    }
    return Hermite(weighted_pos_sum / weight_sum, weighted_normal_sum / weight_sum);
}

func manual_bwd_diff_of_local_centroid(no_diff sp: SurfaceParams, inout x: DifferentialPair<float3>, resgrad: Hermite.Differential) {
    var kernel_weight_accumulator : float = 0;
    var bare_diff_accumulator : float3 = 0;
    var pos_weighted_diff_accumulator : float3 = 0;
    var normal_weighted_diff_accumulator : float3 = 0;
    var pos_weighted_kernel_weight_accumulator : float = 0;
    var normal_weighted_kernel_weight_accumulator : float = 0;
    for (int i = 0; i < sp.point_count; ++i) {
        let posdif = x.v - sp.point_data[i].pos;
        let wdf = fwd_diff(wendland)(sp.support, diffPair(length(posdif), 1.0));
        let df = wdf.d * normalize(posdif);
        let kernel_weight = wdf.v;
        let pos_weight = dot(resgrad.pos, sp.point_data[i].pos);
        let normal_weight = dot(resgrad.normal, sp.point_data[i].normal);
        bare_diff_accumulator += df;
        pos_weighted_diff_accumulator += pos_weight * df;
        normal_weighted_diff_accumulator += normal_weight * df;
        kernel_weight_accumulator += kernel_weight;
        pos_weighted_kernel_weight_accumulator += pos_weight * kernel_weight;
        normal_weighted_kernel_weight_accumulator += normal_weight * kernel_weight;
    }
    let pos_result = (
        kernel_weight_accumulator * pos_weighted_diff_accumulator
        - pos_weighted_kernel_weight_accumulator * bare_diff_accumulator)
        / (kernel_weight_accumulator * kernel_weight_accumulator);
    let normal_result = (
        kernel_weight_accumulator * normal_weighted_diff_accumulator
        - normal_weighted_kernel_weight_accumulator * bare_diff_accumulator)
        / (kernel_weight_accumulator * kernel_weight_accumulator);
    x = diffPair(x.v, pos_result + normal_result);
}

func cofactor(x: float3x3)->float3x3 {
    return float3x3(
        x[1][1]*x[2][2]-x[2][1]*x[1][2], -x[1][0]*x[2][2]+x[2][0]*x[1][2], x[1][0]*x[2][1]-x[2][0]*x[1][1],
        -x[0][1]*x[2][2]+x[0][2]*x[2][1], x[0][0]*x[2][2]-x[0][2]*x[2][0], -x[0][0]*x[2][1]+x[0][1]*x[2][0],
        x[0][1]*x[1][2]-x[0][2]*x[1][1], -x[0][0]*x[1][2]+x[0][2]*x[1][0], x[0][0]*x[1][1]-x[0][1]*x[1][0]
    );
}

func adjugate(x: float3x3)->float3x3 {
    return transpose(cofactor(x));
}

func matrix_inverse(x: float3x3)->float3x3 {
    let adj = adjugate(x);
    let dm = mul(x, adj);
    return adj / dm[0][0];
}

func christoffel(no_diff sp: SurfaceParams, no_diff outer_length: float, x: SituatedPoint)->float3x3[3] {
    let idmat = float3x3(float3(1.0, 0.0, 0.0), float3(0.0, 1.0, 0.0), float3(0.0, 0.0, 1.0));
    var cs : DifferentialPair<float3x3>[3];
    for (int i = 0; i < 3; ++i) {
        cs[i] = fwd_diff(metric)(sp, outer_length, diffPair(x, SituatedPoint.Differential(idmat[i])));
    }
    let here_metric = cs[0].v;
    let inverse_metric = matrix_inverse(here_metric);
    var out : float3x3[3];
    // This can be sped up a little, maybe, by doing the symmetric
    // part first via matmuls and then doing the asymmetric part
    // by scaling and adding whole matrices.
    for (int k = 0; k < 3; ++k) {
        for (int i = 0; i < 3; ++i) {
            for (int j = 0; j < 3; ++j) {
                for (int m = 0; m < 3; ++m) {
                    0.5 * inverse_metric[k][m] * (cs[j].d[m][i] + cs[i].d[m][j] - cs[m].d[i][j]);
                }
            }
        }
    }
    return out;
}

// The surface energy is 1-Lipschitz. To see this, consider
// that lc.normal is an average of normals, so it's at most unit length,
// and you'd get the steepest increase if they were all angled parallel
// with x-lc.pos. x-lc.pos meanwhile is 1-Lipschitz itself, since it gets
// the worst case by lc.pos staying still while x goes away. (lc.pos could
// trail behind x at any speed, so worst case is it stays still - it can't
// recede away from x because our kernel is not deranged.)
// Accordingly, we have that the energy at a point is bounded above by
// its distance from the zero set.
// We also have that it's *consistent* to say that the energy of a distant
// point is equal to its distance from the hyperplane supported at its nearest point.
// To get a safe distance to step our ray, we want basically
// D(x+delta) >= outer_length,
// and we have D(x+delta)>=E(x+delta)>=kernel radius - norm(delta) >= outer_length,
// so long as delta is chosen with norm(delta) <= kernel_radius - outer_length.
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
[ForwardDifferentiable]
func project_onto_surface(no_diff sp: SurfaceParams, x: float3)->float3 {
    let KSTEPS = 10;
    var lambda : float = 0;
    var yPair : DifferentialPair<float3> = diffPair(x, 0);
    bwd_diff(surface_energy)(sp, yPair, 1.0);
    // Invariant: yPair.v approx to basepoint, yPair.d approx to grad f at basepoint
    // x - lambda * yPair.d approx basepoint
    [ForceUnroll]
    for (int k = 0; k < KSTEPS; ++k) {
        yPair = diffPair(x - lambda * yPair.d, yPair.d);
        bwd_diff(surface_energy)(sp, yPair, 1.0);
        lambda += surface_energy(sp, yPair.v)/dot(yPair.d, yPair.d);
    }
    return yPair.v;
}

[ForwardDifferentiable]
func base_and_delta(no_diff sp: SurfaceParams, x: float3)->TR3 {
    let base = project_onto_surface(sp, x);
    return TR3(base, x - base);
}

[ForwardDifferentiable]
func transition_point(no_diff sp: SurfaceParams, no_diff outer_length: float, x: float3)->float3 {
    let bd = base_and_delta(sp, x);
    let new_delta = (outer_length - length(bd.v)) * normalize(bd.v);
    return bd.q + new_delta;
}

[ForwardDifferentiable]
func transition_jacobian(no_diff sp: SurfaceParams, no_diff outer_length: float, x: float3)->float3x3 {
    let d1 = fwd_diff(transition_point)(sp, outer_length, diffPair(x, float3(1, 0, 0))).d;
    let d2 = fwd_diff(transition_point)(sp, outer_length, diffPair(x, float3(0, 1, 0))).d;
    let d3 = fwd_diff(transition_point)(sp, outer_length, diffPair(x, float3(0, 0, 1))).d;

    return float3x3(d1,d2,d3);
}

func transition_tv(no_diff sp: SurfaceParams, no_diff outer_length: float, x: SituatedTR3)->SituatedTR3 {
    let qv = fwd_diff(transition_point)(sp, outer_length, diffPair(x.q, x.v));
    return SituatedTR3(1 - x.region, qv.v, qv.d);
}

func accel_here(no_diff sp: SurfaceParams, no_diff outer_length: float, x: SituatedTR3)->float3 {
    let c = christoffel(sp, outer_length, SituatedPoint(x.region, x.q));
    var out = float3(0);
    out[0] = -dot(x.v, mul(c[0], x.v));
    out[1] = -dot(x.v, mul(c[1], x.v));
    out[2] = -dot(x.v, mul(c[2], x.v));
    return out;
}

func one_outer_step(no_diff sp: SurfaceParams, no_diff outer_length: float, x: SituatedTR3)->SituatedTR3 {
    var energy = surface_energy(sp, x.q);
    if (!isfinite(energy)) {
        energy = sp.support; // kernel radius
    }
    let delta = energy - outer_length;
    if (delta <= 0.25 * outer_length) {
        return x;
    }
    return SituatedTR3(x.region, x.q + delta * x.v, x.v);
}

func phase_vel(no_diff sp: SurfaceParams, no_diff outer_length: float, x: SituatedTR3)->SituatedTR3.Differential {
    return SituatedTR3.Differential(x.v, accel_here(sp, outer_length, x));
}

// RK4
func one_throat_step(no_diff sp: SurfaceParams, no_diff outer_length: float, dt: float, x: SituatedTR3)->SituatedTR3 {
    let k1 = phase_vel(sp, outer_length, x);
    let x1 = SituatedTR3(x.region, x.q + (dt / 2) * k1.q, x.v + (dt / 2) * k1.v);
    let k2 = phase_vel(sp, outer_length, x1);
    let x2 = SituatedTR3(x.region, x.q + (dt / 2) * k2.q, x.v + (dt / 2) * k2.v);
    let k3 = phase_vel(sp, outer_length, x2);
    let x3 = SituatedTR3(x.region, x.q + dt * k2.q, x.v + dt * k2.v);
    let k4 = phase_vel(sp, outer_length, x3);
    return SituatedTR3(x.region, x.q + (dt / 6) * (k1.q + 2 * k2.q + 2 * k3.q + k4.q), x.v + (dt / 6) * (k1.v + 2 * k2.v + 2 * k3.v + k4.v));
}

// RK4
// No early exit or pathology detection right now
// It's not clear how we could implement this nicely at the moment.
// We don't know what the basin of attraction of surface-projection
// looks like, so we don't know when it's safe to take a projection.
// But the point-projection length is what determines whether we've
// entered the throat.
// One alternative is to use a constant surface energy rather than outer_length,
// but surface energy has problems too, due to numerical instability and/or compact
// support of the wendland kernel. We want to use the surface energy only when we're close
// enough that we're guaranteed to get a nice value.
// So we need to gauge our distance to the surface.
// We could do some precomputing, or axis-aligned bounding box shenanigans, in order to
// facilitate this.
func horse_steppin(no_diff sp: SurfaceParams, no_diff outer_length: float, kiter: int, dt: float, x: SituatedTR3)->SituatedTR3 {

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

// REMEMBER:
// float3x3(float3(...), ...) constructs row-by-row
// m[0] is the first row of matrix m, not column
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