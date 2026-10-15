// Night rain on the pond: a still pond seen from its bank at night; now and then a
// drop lands, glints, and sends out a few silver rings that widen and fade. The far
// water is dimmer, as if in mist.
// The screen is laid onto the water plane in perspective; the plane is cut into cells,
// each with a drop now and then, and a pixel looks at the cells around its own.

static const float HORIZON = -0.2;     // a little above the middle; the sky above is left dark
static const float EYE = 1.4;          // the eye's height over the water
static const float CELL = 0.6;
static const float RIPPLE = 5.0;       // seconds a ripple lives

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float pix = 1.0 / resolution.y;
    float t = time;

    // The water under this pixel, and how much water a pixel covers there.
    float below = max(uv.y - HORIZON, 1e-3);
    float z = EYE / below;
    float2 w = float2(uv.x * z, z);
    float span = z / below * pix;          // world size of a pixel along the depth

    float2 cell0 = floor(w / CELL);
    float light = 0.0;
    float glint = 0.0;
    [unroll]
    for (int j = -1; j <= 1; j++)
    {
        [unroll]
        for (int i = -1; i <= 1; i++)
        {
            float2 cell = cell0 + float2(i, j);
            float period = 3.5 + 6.0 * hash12(cell + 0.5);
            float phase = hash12(cell + 3.1) * period;
            float round_ = floor((t + phase) / period);
            float age = t + phase - round_ * period;
            float2 key = cell * 1.7 + round_ * 0.37;
            float falls = step(hash12(key + 9.2), rain);
            float2 at = (cell + 0.25 + 0.5 * float2(hash12(key + 1.3), hash12(key + 5.9))) * CELL;
            float d = length(w - at);
            // A packet of rings travelling outwards, losing strength as it spreads.
            float front = 0.07 * age + 0.01;
            float x = d - front;
            // Three or four rings: the leading one strongest, the inner ones following.
            float wave = cos(x * 160.0) * exp(-x * x / (0.0016 + 0.001 * age)) * smoothstep(0.012, 0.0, x);
            float crest = smoothstep(0.25, 0.95, wave);
            float fade = exp(-age / 1.8) * saturate(1.0 - age / RIPPLE) * saturate(1.0 - d / (CELL * 0.75));
            // Rings closer than a few pixels melt into a faint sheen.
            float sharp = saturate(0.008 / max(span, 1e-5) - 0.3);
            light += falls * fade * lerp(0.25 * saturate(wave), crest, sharp);
            // The drop's own glint where it lands.
            glint += falls * exp(-age / 0.25) * exp(-d / max(0.006, 1.5 * span)) * (0.3 + 0.7 * sharp);
        }
    }

    float mist = exp(-z * 0.17) * step(HORIZON, uv.y);    // the far water fades into the dark
    float3 silver = float3(0.72, 0.82, 1.0);
    // The moon's path on the water: short glints that come and go in a column.
    float pathX = 0.35 * sin(time * 0.004 + 1.0);
    float row = floor(z * 9.0);
    float dash = hash12(float2(row, floor(time * 1.6 + hash12(float2(row, 2.0)) * 7.0)));
    float across = (uv.x - pathX - 0.25 * (hash12(float2(row, 4.0)) - 0.5) * below) / (0.05 + 0.25 * below);
    float glitter = step(0.6, dash) * exp(-across * across * 3.0) * pow(saturate(1.0 - abs(frac(z * 9.0) - 0.5) * 2.0), 2.0);
    glitter *= saturate(1.0 - abs(uv.x - pathX) / (0.06 + 0.5 * below));
    float3 moonlight = float3(0.9, 0.88, 0.8) * glitter * 0.55 * shimmer;
    return silver * (light * 1.5 * shimmer + glint) * mist + moonlight * mist;
}
