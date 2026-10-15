// Bauhaus afternoon: a printed poster of circles, a half disc, a quarter disc and bars on
// cream paper. Each ink is laid like a stencil print: the inks multiply where they
// overlap, each sits a hair off the others, and its density breathes with the paper.

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

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float noise2(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = hash12(i);
    float b = hash12(i + float2(1.0, 0.0));
    float c = hash12(i + float2(0.0, 1.0));
    float d = hash12(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

float sd_box(float2 p, float2 c, float2 h)
{
    float2 d = abs(p - c) - h;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

float sd_circle(float2 p, float2 c, float r)
{
    return length(p - c) - r;
}

// Coverage of a shape from its distance, smoothed over one pixel.
float cover(float d, float pix)
{
    return 1.0 - smoothstep(-pix, pix, d);
}

// Lays an ink over the paper: multiply by the ink's colour where it covers.
float3 ink_on(float3 c, float3 inkc, float amount)
{
    return c * lerp(float3(1.0, 1.0, 1.0), inkc, amount);
}

float3 wall(float2 px)
{
    float pix = 1.0 / resolution.y;
    float2 uv = px / resolution.y;
    // The poster block sits on the right; the left stays bare paper for the icons.
    float2 centre = float2(resolution.x / resolution.y - 0.62, 0.50);
    float2 q = uv - centre;

    // Paper: warm cream with a faint cloudy fibre.
    float fib = noise2(uv * 9.0) * 0.5 + noise2(uv * 70.0) * 0.3 + noise2(uv * 420.0) * 0.2;
    float3 c = float3(0.945, 0.910, 0.840) * (0.975 + 0.04 * fib);

    // Ink density breathes over the sheet, as a stencil print does.
    float dens = ink * (0.90 + 0.10 * noise2(uv * 34.0 + 7.0)) * (0.94 + 0.06 * noise2(uv * 300.0));

    float3 mustard = float3(0.98, 0.72, 0.16);
    float3 brick = float3(0.86, 0.33, 0.22);
    float3 blue = float3(0.13, 0.27, 0.60);
    float3 black = float3(0.10, 0.10, 0.12);

    // Mustard: the sun, a big disc.
    float2 qm = q + float2(0.0015, -0.0010);
    float sun = cover(sd_circle(qm, float2(0.04, -0.08), 0.30), pix);
    c = ink_on(c, mustard, sun * dens);

    // Blue: a quarter disc rising from the lower right, and a thin ring.
    float2 qb = q + float2(-0.0010, 0.0012);
    float2 bc = float2(0.40, 0.34);
    float quarter = cover(sd_circle(qb, bc, 0.36), pix) * step(qb.x, bc.x) * step(qb.y, bc.y);
    float ring = cover(abs(sd_circle(qb, float2(-0.27, -0.25), 0.115)) - 0.006, pix);
    c = ink_on(c, blue, max(quarter, ring) * dens);

    // Brick: a half disc standing on the bar, and a grid of small dots.
    float2 qr = q + float2(0.0008, 0.0015);
    float2 hc = float2(-0.20, 0.215);
    float half_disc = cover(sd_circle(qr, hc, 0.19), pix) * step(qr.y, hc.y);
    float2 g = qr - float2(0.28, -0.40);
    float2 gi = clamp(round(g / 0.045), float2(0.0, 0.0), float2(3.0, 2.0));
    float dots = cover(length(g - gi * 0.045) - 0.0085, pix);
    c = ink_on(c, brick, max(half_disc, dots) * dens);

    // Black: the ground bar, a mast, a small moon and a long hairline to the left.
    float bar = cover(sd_box(q, float2(0.0, 0.2275), float2(0.46, 0.0125)), pix);
    float mast = cover(sd_box(q, float2(0.205, -0.10), float2(0.0035, 0.315)), pix);
    float moon = cover(sd_circle(q, float2(0.34, -0.215), 0.032), pix);
    float hair = cover(sd_box(q, float2(-0.46 - 2.0, 0.2275), float2(2.0, 0.0012)), pix);
    c = ink_on(c, black, max(max(bar, mast), max(moon, hair * 0.55)) * dens);

    // A soft shade of afternoon light across the sheet.
    float2 vg = px / resolution - 0.5;
    c *= 1.0 - 0.10 * dot(vg, vg);
    c *= 0.97 + 0.04 * smoothstep(1.0, 0.0, length(px / resolution - float2(0.8, 0.1)));

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 5.0 / 255.0);
    return c;
}
