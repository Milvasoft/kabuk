// Rose silk: a sheet of silk lying in long diagonal folds, lit from the upper right.
// The cloth is a height field of a few warped waves; its slope gives the normal, and
// the shading mixes a soft diffuse term with silk's long anisotropic sheen.

uint pcg(uint v)
{
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

float grain01(float2 px)
{
    return float(pcg(uint(px.x) + pcg(uint(px.y)))) * (1.0 / 4294967295.0);
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

// The cloth's height: long folds running from lower left to upper right.
float cloth(float2 p)
{
    // Turn so the folds run diagonally.
    float2 q = float2(p.x * 0.80 + p.y * 0.60, -p.x * 0.60 + p.y * 0.80);
    // A slow warp makes the folds swell, bend and pinch along their length.
    q.y += 0.30 * sin(q.x * 1.10 + 0.7) + 0.12 * sin(q.x * 2.30 + 2.1);
    float f1 = sin(q.y * 3.1 + 1.4 * sin(q.x * 1.5 + 0.4));
    float f2 = sin(q.y * 6.7 + q.x * 1.1 + 1.4 + 0.9 * sin(q.x * 2.1));
    float f3 = sin(q.y * 12.5 - q.x * 2.2 + 0.6);
    // Soft crests: rounded valleys, tighter ridges, as cloth falls.
    float h = 0.55 * sqrt(f1 * f1 + 0.05);
    h += 0.20 * f2;
    h += 0.06 * f3 * smoothstep(-0.2, 0.8, f1);
    return h;
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float2 p = uv * 1.35 + float2(0.3, 0.1);
    float e = 2.0 / resolution.y;
    float h = cloth(p);
    float hx = cloth(p + float2(e, 0.0));
    float hy = cloth(p + float2(0.0, e));
    float depth = 0.55;
    float3 n = normalize(float3(-(hx - h) / e * depth, -(hy - h) / e * depth, 1.0));

    // Light from the upper right, low enough to model the folds.
    float3 l = normalize(float3(0.62, -0.50, 0.45));
    float3 v = float3(0.0, 0.0, 1.0);
    float ndl = dot(n, l);
    float wrap = saturate((ndl + 0.25) / 1.25);

    // Silk sheen (Kajiya-Kay): the fibres run along the folds, so the highlight
    // stretches along each crest.
    float3 fibre = normalize(float3(0.80, -0.60, 0.0));
    float3 t = normalize(fibre - n * dot(n, fibre));
    float3 hv = normalize(l + v);
    float th = dot(t, hv);
    float st = sqrt(saturate(1.0 - th * th));
    float sheen = pow(st, 60.0);
    float gloss = pow(saturate(dot(n, hv)), 140.0);
    float rim = pow(1.0 - saturate(n.z), 3.0);

    float3 deep = float3(0.10, 0.02, 0.07);
    float3 mid = float3(0.58, 0.16, 0.25);
    float3 light = float3(0.98, 0.72, 0.62);
    float3 c = lerp(deep, mid, smoothstep(0.0, 0.75, wrap));
    c = lerp(c, light, smoothstep(0.45, 1.0, wrap) * 0.70);
    c += float3(1.0, 0.82, 0.74) * sheen * 0.30 * saturate(ndl + 0.2);
    c += float3(1.0, 0.93, 0.88) * gloss * 0.18;
    c += float3(0.70, 0.25, 0.40) * rim * 0.35;
    // A cool fill from the lower left lifts the shadowed sides of the folds.
    float fill = saturate(dot(n, normalize(float3(-0.6, 0.5, 0.4))));
    c += float3(0.16, 0.08, 0.20) * fill * fill;

    // A gentle falloff towards the left keeps the icon column quiet.
    float side = smoothstep(-1.1, 0.4, uv.x);
    c *= lerp(0.62, 1.0, side);
    float2 vg = px / resolution - 0.5;
    c *= 1.0 - 0.35 * dot(vg, vg);

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
