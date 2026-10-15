// Riso drift: a two-colour risograph print. A sun of graded dots and rolling
// hills in a second ink drift past each other very slowly; where the inks
// overlap they mix to a third colour, the two screens sit at their own
// angles, a little out of step, and the ink lies a little unevenly on the
// paper. With "follow" the inks are the look's accents.

static const float RS_PI = 3.14159265;

float rs_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// Smooth value noise for the uneven ink.
float rs_noise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = rs_hash(i);
    float b = rs_hash(i + float2(1.0, 0.0));
    float c = rs_hash(i + float2(0.0, 1.0));
    float d = rs_hash(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float3 rs_hue(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

// A look colour made into a printing ink: never so light it vanishes on
// the paper.
float3 rs_ink(float3 c)
{
    float lum = dot(c, float3(0.30, 0.59, 0.11));
    return saturate(c * min(1.0, 0.58 / max(lum, 0.02)));
}

// The coverage of one halftone screen at density d (0-1).
float rs_screen(float2 px, float ang, float cell, float d, float ragged)
{
    float c = cos(ang);
    float s = sin(ang);
    float2 q = float2(c * px.x - s * px.y, s * px.x + c * px.y) / cell;
    float2 f = frac(q) - 0.5;
    float r = 0.71 * sqrt(saturate(d));
    float aa = 0.8 / cell;
    return 1.0 - smoothstep(r - aa, r + aa, length(f) + ragged);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;
    float side = smoothstep(0.06, 0.36, uv.x / aspect);
    float unit = resolution.y / 1080.0;

    // The inks.
    float3 inkA = lerp(float3(0.97, 0.35, 0.55), rs_ink(tok_accent), follow);
    float3 a2 = rs_ink(tok_accent2);
    // If the look's two accents are alike, the second ink turns away from the first.
    float same = 1.0 - smoothstep(0.12, 0.25, length(a2 - rs_ink(tok_accent)));
    a2 = lerp(a2, rs_ink(saturate(rs_hue(tok_accent, 2.3))), same);
    float3 inkB = lerp(float3(0.16, 0.40, 0.78), a2, follow);
    float3 paper = float3(0.92, 0.90, 0.85);

    // Ink A: a sun, its dots thinning from the middle out; and a soft diagonal
    // wash along the top.
    float2 sc = float2(aspect * (0.66 + 0.07 * sin(t * 0.017)), 0.44 + 0.07 * sin(t * 0.013 + 1.0));
    float sr = 0.29;
    float sd = length(uv - sc);
    float sun = (1.0 - smoothstep(sr - 0.002, sr + 0.002, sd)) * (0.30 + 0.70 * pow(saturate(1.0 - sd / sr), 0.7));
    float wash = saturate(0.42 - (uv.y + 0.35 * (uv.x / aspect - 0.55))) * 0.9;
    float dA = max(sun, wash * (0.75 + 0.25 * sin(t * 0.011)));

    // Ink B: rolling hills rising from the bottom, densest at the foot; and a
    // small moon on the far right.
    float hx = uv.x * 2.2 + t * 0.006;
    float hill = 0.64 + 0.07 * sin(hx) + 0.035 * sin(hx * 2.3 + 1.7);
    float dB = (1.0 - smoothstep(-0.002, 0.002, hill - uv.y)) * (0.25 + 0.75 * saturate((uv.y - hill) / 0.36));
    float hill2 = 0.80 + 0.05 * sin(uv.x * 3.1 - t * 0.009 + 2.0);
    dB = max(dB, (1.0 - smoothstep(-0.002, 0.002, hill2 - uv.y)) * 0.85);
    float2 mc = float2(aspect * (0.88 + 0.03 * sin(t * 0.021 + 2.0)), 0.20 + 0.04 * cos(t * 0.019));
    float md = length(uv - mc);
    float moon = (1.0 - smoothstep(0.108, 0.112, md)) * (0.2 + 0.8 * saturate((uv.y - mc.y + 0.11) / 0.22));
    dB = max(dB, moon);

    // Uneven ink, and the quiet left.
    float blot = rs_noise(uv * 9.0) * 0.6 + rs_noise(uv * 23.0) * 0.4;
    dA *= side * (0.82 + 0.25 * blot);
    dB *= side * (0.82 + 0.25 * (1.0 - blot));

    // Two screens at their own angles; the second drum is a little out of step.
    float cell = 7.0 * unit * dots;
    float rag = (rs_hash(floor(px / 1.5)) - 0.5) * 0.10;
    float2 off = float2(2.5 + 1.0 * sin(t * 0.007), -1.5) * unit;
    float cA = rs_screen(px, 0.2618, cell, dA, rag);
    float cB = rs_screen(px + off, 1.3090, cell, dB, -rag);

    // Overprint: the inks multiply like real ones.
    float3 col = paper;
    col *= lerp(float3(1.0, 1.0, 1.0), inkA, cA * 0.92);
    col *= lerp(float3(1.0, 1.0, 1.0), inkB, cB * 0.92);
    // The paper's tooth.
    col *= 0.975 + 0.035 * rs_noise(px / (2.2 * unit));
    float2 vv = px / resolution - 0.5;
    col *= 1.0 - 0.12 * dot(vv, vv);
    return col;
}
