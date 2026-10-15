// Ringed planet, night side: a gas giant seen from behind, the sun just past its edge.
// Only a thread of its day side shows; its air glows all round the limb and the rings
// shine with the light coming through them, crossed by the planet's shadow.

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
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

// How much the rings scatter at `x` planet radii from the centre.
float ringDensity(float x)
{
    float c = 0.18 * smoothstep(1.28, 1.34, x) * smoothstep(1.53, 1.5, x);
    float bRing = (0.55 + 0.45 * smoothstep(1.55, 1.85, x)) * smoothstep(1.52, 1.56, x) * smoothstep(1.95, 1.93, x);
    float aRing = 0.6 * smoothstep(2.0, 2.03, x) * smoothstep(2.27, 2.24, x) * smoothstep(0.004, 0.012, abs(x - 2.2));
    float f = 0.35 * exp(-abs(x - 2.36) / 0.004);
    float fine = 0.45 + 0.55 * vnoise(float2(x * 70.0, 0.5)) * (0.5 + 0.5 * vnoise(float2(x * 19.0, 4.0)));
    // a few sharp ringlets
    float lines = smoothstep(0.82, 0.97, vnoise(float2(x * 140.0, 7.0)));
    return (c + bRing + aRing) * (fine * 0.55 + lines * 0.6) + f;
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 dayCol = float3(0.78, 0.93, 1.0);
    float3 airCol = float3(0.25, 0.65, 1.0);
    float3 ringCol = float3(1.0, 0.84, 0.7);
    float3 col = 0.0;

    float R = 0.2;
    float2 P = float2(0.08, -0.02);
    float2 q = p - P;
    float r = length(q);
    float3 L = normalize(float3(-0.55, 0.42, -0.95));
    float3 N = normalize(float3(0.3, 0.93, tilt));

    // The planet.
    float disc = smoothstep(R + pxs, R - pxs, r);
    float zs = sqrt(max(R * R - r * r, 0.0));
    float3 n = float3(q, zs) / R;
    float lit = saturate(dot(n, L));
    float lat = dot(n, N);
    float bands = 0.82 + 0.18 * sin(lat * 26.0 + 2.0 * vnoise(float2(lat * 8.0, n.x * 3.0)));
    col += dayCol * disc * pow(lit, 0.8) * bands * 1.1;

    // Its air, lit from behind: a thin glowing ring round the whole limb, strongest
    // towards the sun.
    float toward = saturate(dot(normalize(q), normalize(L.xy)) * 0.5 + 0.5);
    float phase = 0.12 + 0.88 * pow(toward, 4.0);
    float hOut = max(r - R, 0.0);
    float air = exp(-hOut / 0.0035) * (1.0 - disc) + disc * exp(-max(R - r, 0.0) / 0.0025);
    col += airCol * air * phase * atmosphere * 0.9;
    col += airCol * exp(-hOut / 0.02) * (1.0 - disc) * phase * phase * atmosphere * 0.06;

    // The rings: where this pixel's line of sight meets the ring plane.
    float z = -(N.x * q.x + N.y * q.y) / N.z;
    float3 X = float3(q, z);
    float rho = length(X) / R;
    float dens = ringDensity(rho);
    // hidden behind the planet
    float hidden = disc * step(z, 0.0);
    // the planet's shadow across the rings
    float bb = dot(X, L);
    float cc = dot(X, X) - R * R;
    float shadow = step(bb, 0.0) * smoothstep(0.0, R * R * 0.01, bb * bb - cc);
    // brighter where the light comes nearly straight through, towards the sun
    float through = 0.5 + 0.5 * saturate(dot(normalize(X.xy), normalize(L.xy)) * 0.5 + 0.5);
    float ring = dens * (1.0 - hidden) * (1.0 - shadow) * through;
    col = lerp(col, ringCol * 0.75, saturate(ring * 0.75));

    // A small moon, also seen against the light.
    float2 mq = p - float2(-0.5, 0.24);
    float mr = 0.011;
    float mlen = length(mq);
    float mdisc = smoothstep(mr + pxs, mr - pxs, mlen);
    float3 mn = float3(mq, sqrt(max(mr * mr - mlen * mlen, 0.0))) / mr;
    col += dayCol * mdisc * pow(saturate(dot(mn, L)), 0.8) * 0.9;

    // A sparse field of faint stars, not over the planet or rings.
    float2 g = p * 30.0;
    float2 cell = floor(g);
    float2 off = float2(hash21(cell), hash21(cell + 17.0)) * 0.7 + 0.15;
    float sd = length(g - cell - off) / 30.0;
    float sz = 0.0006 + 0.0009 * hash21(cell + 5.0);
    float clear = smoothstep(R * 1.02, R * 1.1, r) * (1.0 - saturate(dens * 4.0));
    col += float3(0.85, 0.9, 1.0) * step(hash21(cell + 41.0), 0.08) * clear * smoothstep(sz + pxs, sz - pxs, sd) * (0.2 + 0.4 * hash21(cell + 3.0));

    return max(col - 0.003, 0.0);
}
