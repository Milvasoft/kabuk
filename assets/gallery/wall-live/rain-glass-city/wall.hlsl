// Rain on the glass: a night city far out of focus, its lights turned to
// soft discs, seen through a window beaded with rain. Each drop is a tiny
// lens that turns the lights behind it upside down.

float3 rg_hash3(float2 p)
{
    float3 p3 = frac(p.xyx * float3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yxz + 33.33);
    return frac((p3.xxy + p3.yzz) * p3.zyx);
}

// One layer of out-of-focus lights: at most one disc per cell.
float3 rg_bokeh(float2 uv, float scale, float seed, float soft, float t, float warm)
{
    float2 g = uv * scale + float2(t * 0.15 + seed, seed * 3.1);
    float2 cell = floor(g);
    float2 f = frac(g) - 0.5;
    float3 h = rg_hash3(cell + seed * 17.0);
    float3 k = rg_hash3(cell * 1.7 + seed * 5.0 + 11.0);
    float r = 0.14 + 0.16 * h.z;
    float2 c = (h.xy - 0.5) * (0.92 - 2.0 * r);
    float d = length(f - c);
    float disc = smoothstep(r, r - soft, d);
    float rim = smoothstep(r - soft * 2.5, r - soft * 0.5, d) * disc;
    float on = step(0.38, k.x);
    // City colours: sodium amber, warm white, a few reds and cool blues.
    float3 amber = float3(1.00, 0.55, 0.18);
    float3 white = float3(1.00, 0.86, 0.62);
    float3 red = float3(1.00, 0.26, 0.22);
    float3 blue = float3(0.30, 0.58, 1.00);
    float3 col = k.y < 0.45 ? amber : (k.y < 0.70 ? white : (k.y < 0.82 ? red : blue));
    col = lerp(lerp(col, float3(0.55, 0.75, 1.0), 0.35), col, warm);
    float glow = 0.75 + 0.25 * sin(t * 2.0 + k.z * 6.2831853);
    float halo = exp(-max(d - r, 0.0) * 70.0) * (1.0 - disc);
    return col * (disc * 0.6 + rim * 0.6 + halo * 0.12) * on * glow;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.05;

    // Raindrops, fixed on the glass; a few larger ones slide slowly down.
    float2 dg = uv * float2(26.0, 26.0);
    float2 dc = floor(dg);
    float2 df = frac(dg) - 0.5;
    float3 dh = rg_hash3(dc + 41.0);
    float2 dpos = (dh.xy - 0.5) * 0.5;
    float drad = 0.10 + 0.20 * dh.z;
    float2 dd = (df - dpos) / drad;
    float dl = length(dd * float2(1.0, 0.85));
    float drop = smoothstep(1.0, 0.85, dl) * step(1.0 - rain * 0.6, rg_hash3(dc + 7.0).x);

    // Larger runners: one per column, sliding down and wrapping around.
    float colW = 0.045;
    float colI = floor(uv.x / colW);
    float3 rh = rg_hash3(float2(colI, 3.0));
    float ry = frac(rh.x - t * (0.10 + 0.25 * rh.y)) * 1.3 - 0.15;
    float2 rd = float2((frac(uv.x / colW) - 0.5 - (rh.z - 0.5) * 0.4) * colW, uv.y - ry) / (0.012 + 0.008 * rh.y);
    float rl = length(rd * float2(1.0, 0.8));
    float runner = smoothstep(1.0, 0.85, rl) * step(1.0 - rain * 0.5, rh.z);
    float trail = smoothstep(0.35, 0.0, abs(rd.x)) * step(0.0, -rd.y) * exp(rd.y * 0.08) * runner * 0.0;
    trail = smoothstep(0.5, 0.0, abs(rd.x)) * saturate(-rd.y * 0.2) * exp(rd.y * 0.06) * step(1.0 - rain * 0.5, rh.z);

    float lens = max(drop, runner);
    float2 ldir = drop > runner ? dd * drad / 26.0 : rd * 0.02;
    // Inside a drop the scene is seen flipped and shrunk around its centre.
    float2 suv = uv - ldir * lens * 3.5;

    // Night: deep blue above, warm haze over the city below.
    float3 col = lerp(float3(0.020, 0.025, 0.060), float3(0.09, 0.05, 0.08), smoothstep(0.2, 1.0, suv.y));
    col += lerp(float3(0.05, 0.06, 0.10), float3(0.16, 0.08, 0.04), warmth) * smoothstep(0.45, 1.1, suv.y) * lerp(0.4, 1.0, smoothstep(0.0, 0.5, suv.x / aspect));

    // Lights gather in loose bands, like streets at different distances.
    float band = 0.55 + 0.45 * sin(suv.y * 9.0 + sin(suv.x * 1.3) * 1.5);
    float city = smoothstep(0.22, 0.70, suv.y) * band;
    float side = smoothstep(0.05, 0.40, suv.x / aspect);
    float3 lights = rg_bokeh(suv, 2.6, 1.0, 0.03, t, warmth) * 0.65
                  + rg_bokeh(suv, 5.0, 2.0, 0.05, t, warmth) * 0.85
                  + rg_bokeh(suv, 9.0, 3.0, 0.08, t, warmth) * 0.95;
    col += lights * city * side;

    // Drops gather light: a touch brighter inside, a thin dark rim, and a
    // small highlight near the top.
    float rimD = smoothstep(0.7, 1.0, dl) * drop + smoothstep(0.7, 1.0, rl) * runner;
    col *= 1.0 + 0.25 * lens - 0.55 * rimD;
    float2 hd = drop > runner ? dd : rd;
    float spec = smoothstep(0.35, 0.0, length(hd - float2(-0.3, -0.4))) * lens;
    col += float3(0.10, 0.10, 0.12) * spec;
    col += float3(0.010, 0.012, 0.018) * trail;
    return col;
}
