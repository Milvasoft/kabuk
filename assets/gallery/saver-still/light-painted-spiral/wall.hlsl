// Light-painted spiral: a long exposure of coloured lights swung in a widening spiral
// on a tilted disc. Each strand weaves gently around its path; the near side of the
// disc is drawn a little thicker than the far side.

static const float PI = 3.14159265;
static const float TAU = 6.2831853;

// A warm-to-cool palette, coral through gold and teal to violet.
float3 palette(float t)
{
    return saturate(float3(0.62, 0.55, 0.65) + float3(0.42, 0.4, 0.35) * cos(TAU * (float3(1.0, 1.0, 1.0) * t + float3(0.0, 0.28, 0.6))));
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 col = 0.0;

    // Into the plane of the disc: turn, then undo the tilt.
    float2 c = float2(0.02, 0.0);
    float rot = 0.38;
    float2 q = p - c;
    q = float2(q.x * cos(rot) + q.y * sin(rot), -q.x * sin(rot) + q.y * cos(rot));
    float squash = lerp(1.0, 0.42, tilt);
    q.y /= squash;
    float r = max(length(q), 1e-4);
    float phi = atan2(q.y, q.x);

    float a = 0.016;  // radius where the spiral starts
    float b = 0.2;    // how fast it widens per radian
    float lr = log(r / a);
    // pixel size in the disc plane, worst direction
    float pxPlane = pxs / squash;
    // the near half of the tilted disc (screen-lower) is thicker
    float near = 0.5 - 0.5 * sin(phi) * tilt;

    [unroll]
    for (int j = 0; j < 7; j++)
    {
        float fj = float(j);
        float on = saturate(strands - fj);
        float ph = fj / max(strands, 1.0) * TAU;
        // nearest turn of this strand, ignoring the weave
        float u = (lr / b - (phi + ph)) / TAU;
        float k0 = round(u);
        [unroll]
        for (int dk = -1; dk <= 1; dk++)
        {
            float k = k0 + float(dk);
            float th = phi + ph + TAU * k; // the strand's own angle here
            float weave = 0.15 * sin(th * 1.25 + fj * 2.1);
            float target = b * th + weave;
            float dl = lr - target;
            float slope = sqrt(1.0 + b * b);
            float d = r * abs(dl) / slope;
            // length: strands start and end at different turns
            float tStart = 0.4 + 0.3 * frac(fj * 0.618);
            float tEnd = 16.2 + 2.4 * frac(fj * 0.414 + 0.3);
            float life = smoothstep(tStart, tStart + 3.0, th) * smoothstep(tEnd, tEnd - 4.0, th);
            float w = (0.0006 + 0.0011 * near) * (0.6 + 0.4 * smoothstep(0.0, 0.2, r)) / squash;
            float aa = pxPlane;
            float core = smoothstep(w + aa, max(w - aa, 0.0), d) * saturate(w / aa + 0.3);
            float glow = exp(-d / (w * 5.0 + 0.002)) * 0.12;
            // one strand was a blinking light: it left a dashed line
            float sb = sin(th * 14.0);
            float blink = j == 2 ? smoothstep(-0.2, 0.2, sb) : 1.0;
            float blinkSoft = j == 2 ? 0.5 + 0.5 * sb : 1.0;
            float pulse = 0.75 + 0.25 * sin(th * 6.0 + fj * 2.3);
            float3 hue = palette(fj / 7.0 * 0.85 + th * 0.006);
            col += hue * (core * 0.95 * blink + glow * blinkSoft) * life * pulse * on * (0.6 + 0.4 * near);
        }
    }

    // A small white-hot heart where the lights began.
    float rc = length(p - c);
    col += float3(1.0, 0.95, 0.9) * (exp(-rc / 0.006) * 0.5 + exp(-rc / 0.03) * 0.05);

    return max(col - 0.003, 0.0);
}
