// Kintsugi bowl: a black bowl seen from above, broken once and mended with gold. The
// shard seams, a gold-filled chip in the rim, and the rim and foot ring in faint clay.

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float2 hash22(float2 p)
{
    float3 a = frac(p.xyx * float3(123.34, 234.34, 345.65));
    a += dot(a, a + 34.45);
    return frac(float2(a.x * a.y, a.y * a.z));
}

float vnoise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash21(i);
    float b = hash21(i + float2(1.0, 0.0));
    float c = hash21(i + float2(0.0, 1.0));
    float d = hash21(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, f.x), lerp(c, d, f.x), f.y);
}

static const float TAU = 6.2831853;

// The angle (around the point of impact) of radial crack `k` at distance `rho`.
float crackAngle(float k, float base, float rho)
{
    return base + 0.22 * (vnoise(float2(rho * 6.0, k * 7.3)) - 0.5) + 0.05 * (vnoise(float2(rho * 27.0, k * 3.1)) - 0.5);
}

float wrapAngle(float a)
{
    return a - TAU * round(a / TAU);
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float rOut = 0.37;
    float rIn = 0.352;
    float rFoot = 0.15;
    float r = length(p);
    float ang = atan2(p.y, p.x);

    float3 clay = float3(0.13, 0.112, 0.1);
    float3 goldLo = float3(0.55, 0.34, 0.1);
    float3 goldHi = float3(1.0, 0.82, 0.45);
    float3 col = 0.0;

    // The rim (two edges) and the foot ring, as faint lines.
    float lw = 1.2 * pxs;
    col += clay * smoothstep(lw + pxs, lw - pxs, abs(r - rOut));
    col += clay * 0.8 * smoothstep(lw + pxs, lw - pxs, abs(r - rIn));
    col += clay * 0.55 * smoothstep(lw + pxs, lw - pxs, abs(r - rFoot));

    // The seams: cracks running out from one point of impact to the rim, and a few
    // crossing between them.
    float2 warp2 = float2(vnoise(p * 31.0 + 5.0), vnoise(p * 31.0 + 41.0)) - 0.5;
    float2 q = p + warp2 * 0.004;
    float2 imp = float2(0.07, -0.05);
    float2 v = q - imp;
    float rho = length(v);
    float phi = atan2(v.y, v.x);
    float bases[7] = { 0.35, 2.3, 4.2, 1.35, 5.3, 3.2, 2.85 };
    float starts[7] = { 0.0, 0.0, 0.0, 0.03, 0.0, 0.05, 0.12 };
    float visible = 3.0 + shards * 4.0;
    float d = 1.0;
    [unroll]
    for (int k = 0; k < 7; k++)
    {
        float fk = float(k);
        float th = crackAngle(fk, bases[k], rho);
        float dk = rho * abs(wrapAngle(phi - th));
        float on = step(fk + 0.5, visible) * smoothstep(starts[k], starts[k] + 0.04, rho);
        dk = on > 0.0 ? dk / max(on, 0.05) : 1.0;
        d = min(d, dk);
    }
    // Arcs between neighbouring cracks: (crack a, crack b, distance from impact).
    float3 arcs[3] = { float3(0.0, 3.0, 0.15), float3(2.0, 4.0, 0.22), float3(1.0, 5.0, 0.24) };
    [unroll]
    for (int m = 0; m < 3; m++)
    {
        float ka = arcs[m].x;
        float kb = arcs[m].y;
        float rc = arcs[m].z;
        float ta = crackAngle(ka, bases[(int)ka], rc);
        float tb = crackAngle(kb, bases[(int)kb], rc);
        float span = wrapAngle(tb - ta);
        float u = wrapAngle(phi - ta) / span;
        float rr = rc + 0.03 * (vnoise(float2(u * 3.0, 9.0 + float(m) * 5.0)) - 0.5) * sin(saturate(u) * 3.14159);
        float da = abs(rho - rr);
        float inside = step(0.0, u) * step(u, 1.0);
        float onArc = step(float(m) + 0.5, shards * 3.0 + 0.6);
        d = min(d, inside * onArc > 0.0 ? da : 1.0);
    }
    float w = 0.0011 + 0.0042 * vnoise(q * 9.0) * vnoise(q * 3.0 + 7.0);
    w += 0.004 * exp(-rho / 0.012);
    float seam = smoothstep(w + pxs, w - pxs, d) * smoothstep(rIn + 0.001, rIn - 0.001, r);
    // Gold leaf: brushed, uneven, brighter in the middle of the seam.
    float prof = saturate(1.0 - d / max(w, pxs));
    float leaf = 0.55 * vnoise(q * 160.0) + 0.45 * vnoise(q * 45.0 + 3.0);
    float3 gc = lerp(goldLo, goldHi, saturate(0.25 + 0.55 * leaf + 0.45 * prof));
    float sheen = 0.68 + 0.5 * vnoise(q * 13.0 + 2.0);
    col = lerp(col, gc * gold * sheen, seam);
    // a warm breath around the gold
    col += goldLo * 0.06 * gold * exp(-max(d - w, 0.0) / 0.004) * smoothstep(rIn, rIn - 0.01, r) * (1.0 - seam);

    // A chip in the rim, filled with gold.
    float a1 = 0.62;
    float da = ang - a1;
    float chipDepth = 0.035 * saturate(1.0 - pow(abs(da) / 0.22, 2.0)) * (0.8 + 0.4 * vnoise(float2(ang * 30.0, 1.0)));
    float chip = smoothstep(rOut - chipDepth - pxs, rOut - chipDepth + pxs, r) * smoothstep(rOut + pxs, rOut - pxs, r) * step(0.0, chipDepth - 0.002);
    float3 cc = lerp(goldLo, goldHi, saturate(0.35 + 0.8 * vnoise(float2(ang * 90.0, r * 300.0)) * vnoise(p * 70.0)));
    col = lerp(col, cc * gold * (0.8 + 0.3 * vnoise(p * 40.0)), chip);

    return max(col - 0.002, 0.0);
}
