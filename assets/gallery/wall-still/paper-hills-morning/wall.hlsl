// Paper hills: seven sheets of cut paper stacked into a morning valley. Each sheet is a
// hill line; the sheet in front casts a soft shadow on the one behind it, mist pales the
// far sheets, and every sheet carries a faint paper fibre.

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

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float noise1(float x)
{
    float i = floor(x);
    float f = frac(x);
    float u = f * f * (3.0 - 2.0 * f);
    return lerp(hash11(i), hash11(i + 1.0), u) * 2.0 - 1.0;
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

// The top edge of sheet i (0 = farthest), in units of the screen height from the bottom.
float ridge(float x, float i)
{
    float k = i / 6.0;
    float base = lerp(0.56, 0.13, pow(k, 0.85));
    float amp = lerp(0.05, 0.15, k);
    float f = lerp(1.7, 0.8, k);
    float s = i * 17.31;
    float r = noise1(x * f + s) * 0.70 + noise1(x * f * 2.3 + s * 1.7) * 0.25 + noise1(x * f * 6.1 + s) * 0.05;
    // The valley opens towards the sun, on the right; the left stays low and calm.
    r = r * amp + 0.06 * k * sin(x * 0.9 + i * 2.1);
    // A cut paper edge is not perfectly smooth.
    r += noise1(x * 90.0 + s) * 0.0012;
    return base + r;
}

// A row of cut paper pines standing on sheet i, as a coverage 0-1 above its ridge.
float pines(float x, float y, float i, float pix)
{
    float cell = 0.022 + 0.006 * i;
    float cov = 0.0;
    float ci = floor(x / cell);
    [unroll] for (int j = -1; j <= 1; j++)
    {
        float id = ci + float(j);
        float h = hash11(id * 3.17 + i * 11.0);
        float there = step(0.35, h) * step(0.0, sin(id * cell * 2.3 + i * 1.7) + 0.2);
        float cx = (id + 0.25 + 0.5 * hash11(id + i * 7.0)) * cell;
        float hgt = cell * (1.6 + 1.4 * hash11(id * 1.7 + i));
        // Stand on the ridge where the trunk is, and stay out of the icon column.
        float r = ridge(cx, i) - 0.004;
        there *= step(0.22, cx);
        float t = (y - r) / hgt;
        float ft = frac(t * 3.0);
        float hw = cell * 0.42 * (1.0 - 0.55 * t) * (1.0 - 0.85 * ft) * (0.8 + 0.4 * h);
        float inx = smoothstep(-pix, pix, hw - abs(x - cx));
        cov = max(cov, inx * step(-0.03, t) * step(t, 0.999) * there);
    }
    return cov;
}

float3 wall(float2 px)
{
    float aspect = resolution.x / resolution.y;
    float x = px.x / resolution.y;
    float y = 1.0 - px.y / resolution.y;
    float pix = 1.0 / resolution.y;

    // The sky: pale mint above, warm cream at the horizon, and a low morning sun.
    float2 sunp = float2(aspect * 0.70, lerp(0.40, 0.78, sun));
    float3 sky = lerp(float3(0.99, 0.90, 0.80), float3(0.74, 0.87, 0.86), smoothstep(0.35, 1.0, y));
    float sd = length(float2(x, y) - sunp);
    sky += float3(1.0, 0.80, 0.60) * 0.22 * exp(-sd * 4.0);
    float disc = 1.0 - smoothstep(0.085 - pix, 0.085 + pix, sd);
    float3 c = lerp(sky, float3(1.0, 0.97, 0.90), disc);

    // Paper sheets, back to front.
    float3 farc = float3(0.70, 0.83, 0.81);
    float3 nearc = float3(0.06, 0.23, 0.25);
    float fibre = noise2(float2(x * 140.0, y * 110.0)) * 0.5 + noise2(float2(x * 700.0, y * 650.0)) * 0.5;
    [unroll] for (int i = 0; i < 7; i++)
    {
        float fi = float(i);
        float r = ridge(x, fi);
        float inside = 1.0 - smoothstep(r - pix, r + pix, y);
        if (i == 3 || i == 5)
        {
            inside = max(inside, pines(x, y, fi, pix));
        }
        float k = fi / 6.0;
        float3 sheet = lerp(farc, nearc, pow(k, 0.85));
        sheet *= float3(1.0 + 0.05 * sin(fi * 2.4), 1.0, 1.0 - 0.04 * sin(fi * 2.4));
        // Light falls from the sun's side: sheets warm a little towards it.
        float glow = exp(-abs(x - sunp.x) * 1.2) * (1.0 - k);
        sheet += float3(0.10, 0.06, 0.02) * glow;
        // The sheet darkens a little away from its own edge, as paper in a box does.
        sheet *= 1.0 - 0.06 * smoothstep(0.0, 0.25, r - y);
        // A lit paper edge.
        sheet += float3(0.10, 0.09, 0.07) * (1.0 - k * 0.6) * exp(-max(r - y, 0.0) * resolution.y * 0.35) * step(y, r);
        sheet *= 0.98 + 0.035 * fibre;
        // The next sheet in front casts a soft shadow onto this one.
        float rn = ridge(x - 0.010, fi + 1.0) + 0.006;
        float shade = (i < 6) ? exp(-max(y - rn, 0.0) * 30.0) : 0.0;
        sheet = lerp(sheet, sheet * float3(0.55, 0.68, 0.70), shade * 0.75);
        c = lerp(c, sheet, inside);
    }

    // A gentle vignette.
    float2 vg = px / resolution - 0.5;
    c *= 1.0 - 0.22 * dot(vg, vg);

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
