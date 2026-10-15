// Smoke rings of light: now and then a ring of luminous smoke is puffed up from below;
// it rises, widens and rolls, its edge fraying into wisps, until it thins away into
// the dark. Each ring has its own colour of ink.

static const float EVERY = 9.0;       // seconds between two rings
static const int RINGS = 3;           // rings alive at once
static const float LIFE = 27.0;

float hash12(float2 p)
{
    return frac(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

float noise(float2 p)
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

float fbm(float2 p)
{
    float v = 0.5 * noise(p);
    v += 0.25 * noise(p * 2.03 + 17.0);
    v += 0.125 * noise(p * 4.01 + 41.0);
    return v / 0.875;
}

// Two octaves: enough to push the ring about.
float swirl(float2 p)
{
    return (2.0 * noise(p) + noise(p * 2.03 + 17.0)) / 3.0;
}

static const float3 INK[4] = {
    float3(1.0, 0.55, 0.3),     // ember
    float3(0.45, 0.85, 0.9),    // sea glass
    float3(0.85, 0.5, 0.85),    // lilac
    float3(0.95, 0.8, 0.5),     // straw
};

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * drift + 20.0;
    float newest = floor(t / EVERY);
    float3 col = 0;

    [unroll]
    for (int i = 0; i < RINGS; i++)
    {
        float id = newest - i;
        float age = t - id * EVERY;
        float h1 = hash12(float2(id, 1.7));
        float h2 = hash12(float2(id, 8.3));
        // Rising, widening, thickening; drifting a little sideways.
        float2 c = float2((h1 - 0.5) * max(aspect - 0.9, 0.3) + 0.04 * age * (h2 - 0.5),
                          0.62 - 0.042 * age);
        float R = 0.07 + 0.0085 * age;
        float tube = 0.013 + 0.003 * age;
        float tilt = 0.3 + 0.08 * h2;                   // seen a little from below
        float2 p = (uv - c) / R;
        p.y /= tilt;
        // The edge frays: the place is pushed about by slow turbulence before measuring.
        float2 w = float2(swirl(p * 2.2 + float2(age * 0.15, id)), swirl(p * 2.2 + float2(id, -age * 0.15) + 5.0)) - 0.5;
        p += w * (0.14 + 0.018 * age);
        float rho = length(p);
        float s = (rho - 1.0) * R / tube;               // across the tube, in tube widths
        float ang = atan2(p.y, p.x);
        // The tube rolls: wisps travel round it, and along the ring they come and go.
        float wisps = fbm(float2(ang * 3.0 + id * 3.1, s * 1.2 - age * 0.6));
        float density = exp(-s * s * 1.1) * (0.3 + 1.1 * wisps * wisps);
        // The near half is a little brighter, and the ends, seen through more smoke, too.
        float front = (0.75 + 0.25 * saturate(p.y * 2.0 + 0.5)) * (0.75 + 0.5 * p.x * p.x);
        float alive = smoothstep(0.0, 2.5, age) * exp(-age / 12.0) * smoothstep(LIFE, LIFE - 6.0, age);
        float3 ink = INK[(int)fmod(abs(id), 4.0)];
        col += ink * density * front * alive * 1.1 * haze;
    }
    return col;
}
