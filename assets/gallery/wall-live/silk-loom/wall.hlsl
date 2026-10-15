// Silk loom: two families of glowing threads woven over and under each
// other, the cloth lifting gently as if in a breeze while soft light runs
// along each thread.

static const float SL_TAU = 6.2831853;

float3 sl_hue(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

// Nearest thread of one family. x runs along the threads, y across them.
// Returns (distance across in thread spacings, thread index, light).
float3 sl_family(float2 st, float t, float seed)
{
    float k0 = floor(st.y + 0.5);
    float best = 1e3;
    float idx = k0;
    [unroll] for (int j = -1; j <= 1; j++)
    {
        float k = k0 + j;
        float off = 0.32 * sin(st.x * 0.35 + k * 0.45 + t * 0.6 + seed)
                  + 0.12 * sin(st.x * 0.9 - k * 0.8 - t * 0.4 + seed * 2.0);
        float d = abs(st.y - k - off);
        if (d < best)
        {
            best = d;
            idx = k;
        }
    }
    // Light travels slowly along each thread.
    float run = 0.55 + 0.45 * sin(st.x * 0.22 - t * 1.3 + idx * 1.7 + seed);
    return float3(best, idx, run);
}

// A silk ribbon seen across its width: rounded, with a moving sheen.
float3 sl_ribbon(float d, float w, float3 base, float run)
{
    float x = saturate(d / w);
    float body = sqrt(1.0 - x * x);
    float3 c = base * (0.25 + 0.75 * body);
    c += lerp(base, float3(1.0, 0.95, 0.88), 0.5) * pow(body, 6.0) * run * 0.35;
    return c;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.08;
    float n = threads * 0.36;
    float pxs = n / resolution.y;
    float w = 0.36;

    // The cloth: one family leans up, the other down.
    float2 a = float2(0.88, 0.47);
    float2 b = float2(0.88, -0.47);
    float2 sa = float2(dot(uv, a), dot(uv, float2(-a.y, a.x))) * n;
    float2 sb = float2(dot(uv, b), dot(uv, float2(-b.y, b.x))) * n;
    float3 fa = sl_family(sa, t, 0.0);
    float3 fb = sl_family(sb, t, 3.7);

    float inA = smoothstep(w + pxs, w - pxs, fa.x);
    float inB = smoothstep(w + pxs, w - pxs, fb.x);

    // Plain weave: at each crossing one ribbon passes over the other.
    float aOver = frac((fa.y + fb.y) * 0.5) < 0.25 ? 1.0 : 0.0;

    // Champagne weft over a midnight warp, each shifting a little in tone.
    float3 gold = float3(0.62, 0.50, 0.34);
    float3 rose = float3(0.58, 0.40, 0.36);
    float3 teal = float3(0.08, 0.13, 0.24);
    float3 ink = float3(0.11, 0.11, 0.22);
    float ga = 0.5 + 0.5 * sin(fa.y * 0.9 + t * 0.3);
    float gb = 0.5 + 0.5 * sin(fb.y * 0.7 - t * 0.25);
    float3 colA = sl_ribbon(fa.x, w, sl_hue(lerp(gold, rose, ga), hue * SL_TAU), fa.z);
    float3 colB = sl_ribbon(fb.x, w, sl_hue(lerp(teal, ink, gb), hue * SL_TAU), fb.z);

    float3 bg = lerp(float3(0.030, 0.025, 0.045), float3(0.050, 0.030, 0.050), uv.y);
    float3 col = bg;
    // The lower ribbon first, darkened where the upper one shades it.
    float3 under = aOver > 0.5 ? colB : colA;
    float inU = aOver > 0.5 ? inB : inA;
    float3 upper = aOver > 0.5 ? colA : colB;
    float inO = aOver > 0.5 ? inA : inB;
    float dO = aOver > 0.5 ? fa.x : fb.x;
    float shade = 1.0 - 0.6 * smoothstep(w * 1.6, w, dO);
    col = lerp(col, under * shade, inU);
    col *= 1.0 - 0.5 * smoothstep(w * 1.5, w, dO) * (1.0 - inO) * (1.0 - inU);
    col = lerp(col, upper, inO);

    // Let the cloth fade into the dark where the icons sit.
    float side = smoothstep(0.02, 0.38, uv.x / aspect);
    return lerp(bg, col, side);
}
