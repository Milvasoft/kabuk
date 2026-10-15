// Low poly dusk: three mountain ranges and a dusk sky, cut into flat triangles. The
// screen is a jittered triangle mesh; each triangle takes one colour: the sky at its
// centre, or a mountain face lit by the setting sun through the triangle's own normal.

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

float hash11(float x)
{
    x = frac(x * 0.1031);
    x *= x + 33.33;
    x *= x + x;
    return frac(x);
}

float2 hash22(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * float3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.xx + p3.yz) * p3.zy);
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

// The x of corner k of a range's polyline.
float corner(float k, float layer, float w)
{
    return (k + 0.35 * (hash11(k * 1.37 + layer * 31.0) - 0.5)) * w;
}

// A range of peaks: a polyline that alternates peaks (even corners) and saddles (odd
// corners). Returns the ridge height at x, and the corner numbers of the peaks to the
// left (.y) and right (.z) of x.
float3 range(float x, float layer)
{
    float w = lerp(0.24, 0.36, layer / 2.0);
    float k0 = floor(x / w);
    // After jitter the corner left of x may sit in the previous or the next cell.
    float k = (x < corner(k0, layer, w)) ? k0 - 1.0 : ((x >= corner(k0 + 1.0, layer, w)) ? k0 + 1.0 : k0);
    float xa = corner(k, layer, w);
    float xb = corner(k + 1.0, layer, w);
    float sz = lerp(0.85, 1.25, layer / 2.0);
    float pa = frac(k * 0.5) < 0.25 ? 1.0 : 0.0;
    float ha = lerp(lerp(0.0, 0.07, hash11(k * 2.91 + layer)), lerp(0.10, 0.27, hash11(k * 2.91 + layer)), pa) * sz;
    float hb = lerp(lerp(0.10, 0.27, hash11((k + 1.0) * 2.91 + layer)), lerp(0.0, 0.07, hash11((k + 1.0) * 2.91 + layer)), pa) * sz;
    float r = lerp(ha, hb, saturate((x - xa) / (xb - xa)));
    float base = lerp(0.40, 0.06, layer / 2.0);
    float pl = pa > 0.5 ? k : k - 1.0;
    return float3(base + r, pl, pl + 2.0);
}

float3 sky_at(float y, float x, float2 sunp)
{
    float3 top = float3(0.09, 0.09, 0.24);
    float3 mid = float3(0.44, 0.24, 0.46);
    float3 low = float3(0.98, 0.55, 0.38);
    float3 c = lerp(low, mid, smoothstep(0.28, 0.62, y));
    c = lerp(c, top, smoothstep(0.60, 1.05, y));
    float d = length(float2(x, y) - sunp);
    c += float3(1.0, 0.55, 0.30) * 0.35 * exp(-d * 3.5);
    return c;
}

// Which range a point belongs to (-1 = sky), front-most first.
float which(float2 p)
{
    float l = -1.0;
    [unroll] for (int i = 0; i < 3; i++)
    {
        float r = range(p.x, float(i)).x;
        l = (p.y < r) ? float(i) : l;
    }
    return l;
}

// The surface of a mountain face as a depth: each peak is a pyramid whose front ridge
// runs down at its own slant; the face belongs to whichever pyramid stands nearer.
float depth_at(float2 p, float layer)
{
    float w = lerp(0.24, 0.36, layer / 2.0);
    float3 r = range(p.x, layer);
    float d = r.x - p.y;
    float hl = hash11(r.y * 5.17 + layer) - 0.5;
    float sl = sign(hl) * (0.40 + 0.8 * abs(hl));
    float hr = hash11(r.z * 5.17 + layer) - 0.5;
    float sr = sign(hr) * (0.40 + 0.8 * abs(hr));
    // Each peak's front ridge runs straight down from its tip.
    float sz = lerp(0.85, 1.25, layer / 2.0);
    float base = lerp(0.40, 0.06, layer / 2.0);
    float tl = base + lerp(0.10, 0.27, hash11(r.y * 2.91 + layer)) * sz;
    float tr = base + lerp(0.10, 0.27, hash11(r.z * 2.91 + layer)) * sz;
    float zl = -abs(p.x - corner(r.y, layer, w) + (tl - p.y) * sl);
    float zr = -abs(p.x - corner(r.z, layer, w) + (tr - p.y) * sr);
    float z = max(zl, zr) * 1.2 - d * 0.35;
    z += 0.02 * sin(p.x * 23.0 + layer) * sin(p.y * 19.0 - layer * 2.0);
    return z;
}

float3 wall(float2 px)
{
    float aspect = resolution.x / resolution.y;
    float2 p = float2(px.x / resolution.y, 1.0 - px.y / resolution.y);
    float2 sunp = float2(aspect * 0.74, 0.47);

    // The mesh: a jittered grid, each square cut along an alternating diagonal.
    float s = 0.058 * facet;
    float2 cs = float2(s, s * 0.82);
    float2 gc = floor(p / cs);
    float2 ta = p;
    float2 tb = p;
    float2 tc = p;
    [unroll] for (int j = -1; j <= 1; j++)
    {
        [unroll] for (int i = -1; i <= 1; i++)
        {
            float2 g = gc + float2(i, j);
            float2 v00 = (g + 0.32 * (hash22(g) - 0.5)) * cs;
            float2 v10 = (g + float2(1, 0) + 0.32 * (hash22(g + float2(1, 0)) - 0.5)) * cs;
            float2 v01 = (g + float2(0, 1) + 0.32 * (hash22(g + float2(0, 1)) - 0.5)) * cs;
            float2 v11 = (g + float2(1, 1) + 0.32 * (hash22(g + float2(1, 1)) - 0.5)) * cs;
            bool flip = frac((g.x + g.y) * 0.5) > 0.25;
            float2 a1 = v00;
            float2 b1 = v10;
            float2 c1 = flip ? v01 : v11;
            float2 a2 = flip ? v10 : v00;
            float2 b2 = v11;
            float2 c2 = v01;
            // Signs of the edges tell if p is inside.
            float e0 = (b1.x - a1.x) * (p.y - a1.y) - (b1.y - a1.y) * (p.x - a1.x);
            float e1 = (c1.x - b1.x) * (p.y - b1.y) - (c1.y - b1.y) * (p.x - b1.x);
            float e2 = (a1.x - c1.x) * (p.y - c1.y) - (a1.y - c1.y) * (p.x - c1.x);
            bool in1 = (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0);
            float f0 = (b2.x - a2.x) * (p.y - a2.y) - (b2.y - a2.y) * (p.x - a2.x);
            float f1 = (c2.x - b2.x) * (p.y - b2.y) - (c2.y - b2.y) * (p.x - b2.x);
            float f2 = (a2.x - c2.x) * (p.y - c2.y) - (a2.y - c2.y) * (p.x - c2.x);
            bool in2 = (f0 >= 0.0 && f1 >= 0.0 && f2 >= 0.0) || (f0 <= 0.0 && f1 <= 0.0 && f2 <= 0.0);
            ta = in1 ? a1 : (in2 ? a2 : ta);
            tb = in1 ? b1 : (in2 ? b2 : tb);
            tc = in1 ? c1 : (in2 ? c2 : tc);
        }
    }
    float2 g0 = (ta + tb + tc) / 3.0;
    // Membership is per pixel, so ridges stay straight; the facet is per triangle.
    float layer = which(p);
    float tri_id = hash11(dot(floor(g0 * 997.0), float2(1.0, 57.0)));

    float3 c;
    if (layer < 0.0)
    {
        c = sky_at(g0.y, g0.x, sunp) * (0.985 + 0.03 * tri_id);
    }
    else
    {
        // The face's normal from the three corners lifted to the mountain's surface.
        float3 A = float3(ta, depth_at(ta, layer));
        float3 B = float3(tb, depth_at(tb, layer));
        float3 C = float3(tc, depth_at(tc, layer));
        float3 n = normalize(cross(B - A, C - A));
        n *= sign(n.z);
        float3 l = normalize(float3(0.85, 0.15, 0.50));
        float lit = saturate(dot(n, l));
        float k = layer / 2.0;
        float3 shadow = lerp(float3(0.36, 0.25, 0.47), float3(0.14, 0.10, 0.23), k);
        float3 warm = lerp(float3(0.92, 0.55, 0.50), float3(0.55, 0.24, 0.30), k);
        c = lerp(shadow, warm, smoothstep(0.1, 0.9, lit));
        // Haze: far ranges sink into the glow of the horizon.
        c = lerp(c, sky_at(0.30, g0.x, sunp), (1.0 - k) * 0.35);
        c *= 0.97 + 0.06 * tri_id;
    }

    // A few small stars high in the sky, kept away from the triangles' edges.
    float2 sg = floor(px / 90.0);
    float2 sr = hash22(sg + 7.0);
    float2 sp = (sg + 0.2 + 0.6 * sr) * 90.0;
    float star = step(0.86, hash22(sg * 1.3).x) * smoothstep(2.4, 0.6, length(px - sp) * 2160.0 / resolution.y * 0.5);
    c += float3(1.0, 0.92, 0.85) * star * 0.5 * smoothstep(0.66, 0.95, p.y) * (layer < 0.0 ? 1.0 : 0.0);

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
