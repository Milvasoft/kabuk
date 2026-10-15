// Aurora shore: slow aurora curtains over a dark mountain line, mirrored
// in a still lake. The curtains hang right of centre; the left stays dark.

float as_hash(float n)
{
    return frac(sin(n) * 43758.5453);
}

// Smooth 1D value noise.
float as_noise(float x)
{
    float i = floor(x);
    float f = frac(x);
    f = f * f * (3.0 - 2.0 * f);
    return lerp(as_hash(i), as_hash(i + 1.0), f);
}

// Hue rotation of an RGB colour around the grey axis.
float3 as_hue(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

// The aurora's light at (x, h): h is the height above the horizon.
float3 as_aurora(float x, float h, float t, float aspect)
{
    float3 acc = 0.0;
    [unroll] for (int i = 0; i < 3; i++)
    {
        float fi = i;
        float xf = x + 0.09 * sin(x * 1.7 + t * 0.21 + fi * 2.1) + 0.04 * sin(x * 4.3 - t * 0.17 + fi);
        float base = 0.075 + 0.07 * fi + 0.045 * sin(x * 1.3 + t * 0.13 + fi * 1.9) + 0.03 * sin(x * 3.1 - t * 0.19 + fi) + 0.012 * sin(x * 7.3 + t * 0.29);
        float d = h - base;
        float lower = smoothstep(-0.012, 0.006, d);
        float fall = exp(-max(d, 0.0) * (7.0 - fi * 1.5));
        float rays = 0.35 + 0.65 * as_noise(xf * 70.0 + fi * 31.0 + t * 0.6) * as_noise(xf * 17.0 - fi * 7.0 - t * 0.25);
        float span = smoothstep(0.30, 0.55, x / aspect + 0.08 * fi) * (1.0 - smoothstep(0.85, 1.05, x / aspect + 0.05 * fi));
        float3 low = float3(0.25, 1.0, 0.62);
        float3 high = lerp(float3(0.20, 0.55, 1.0), float3(0.75, 0.30, 0.95), fi * 0.5);
        float3 c = lerp(low, high, saturate(d * 4.0 + fi * 0.25));
        acc += c * lower * fall * rays * span * (1.0 - fi * 0.22);
    }
    return acc;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.25;
    float horizon = 0.74;

    // Mountain line: soft ridges a little above the water.
    float ridge = horizon - 0.018 - 0.045 * pow(abs(sin(uv.x * 1.7 + 0.7)), 2.0) * (0.5 + 0.5 * sin(uv.x * 0.6 + 2.0))
                - 0.010 * sin(uv.x * 9.0 + 0.4) * sin(uv.x * 3.1) - 0.004 * sin(uv.x * 31.0 + 1.3);

    float y = uv.y;
    bool water = y > horizon;
    // The lake mirrors the sky, with a slow ripple.
    float my = water ? 2.0 * horizon - y : y;
    float mx = uv.x + (water ? 0.005 * sin(y * 70.0 + 1.5 * sin(uv.x * 2.3) - t * 0.8) * saturate((y - horizon) * 10.0) : 0.0);
    float h = horizon - my;

    float3 skyTop = float3(0.010, 0.016, 0.040);
    float3 skyLow = float3(0.020, 0.060, 0.085);
    float3 sky = lerp(skyLow, skyTop, saturate(h * 1.6));

    float3 light = as_hue(as_aurora(mx, h, t, aspect), hue * 6.2831853) * glow;
    sky += light;
    // The aurora lights the haze just above the hills.
    float haze = exp(-max(h, 0.0) * 14.0) * smoothstep(0.25, 0.65, mx / aspect) * (1.0 - smoothstep(0.9, 1.1, mx / aspect));
    sky += as_hue(float3(0.02, 0.07, 0.05), hue * 6.2831853) * haze * glow;

    // A few faint fixed stars high up, where the aurora is thin.
    float2 cell = floor(px / resolution.y * 260.0);
    float star = step(0.9965, as_hash(cell.x * 13.7 + cell.y * 91.3));
    sky += star * 0.35 * smoothstep(0.12, 0.35, uv.x / aspect) * saturate(h * 3.0 - 0.6) * (1.0 - saturate(length(light) * 3.0)) * (water ? 0.0 : 1.0);

    float3 col = sky;
    if (water)
        col = sky * 0.45 + float3(0.004, 0.010, 0.016);

    // Mountains are black-blue against the sky, and their mirror on the water.
    float my2 = water ? 2.0 * horizon - y : y;
    float hill = smoothstep(ridge - 0.0015, ridge + 0.0015, my2);
    float3 rock = float3(0.008, 0.012, 0.022) + light * 0.04;
    col = lerp(col, rock, hill * (water ? 0.85 : 1.0));

    // A thin bright line where land meets water.
    col += float3(0.05, 0.12, 0.10) * glow * exp(-abs(y - horizon) * 900.0) * smoothstep(0.3, 0.7, uv.x / aspect);
    return col;
}
