// Comet calligraphy: a lone comet writes slow cursive strokes across the dark with a
// broad nib; the ink cools from white gold to ember red and fades into the black.
// The path is two slow waves plus a turning loop; each wave is kept as a phasor and
// stepped back along the trail by a fixed rotation, so the trail costs no sines. A
// coarse pass over chunks of the trail finds the two nearest; only those are drawn
// segment by segment.

static const int CHUNKS = 30;          // coarse pieces of the trail
static const int FINE = 4;             // segments in a chunk
static const float STEP = 0.2;         // seconds of path per segment
// Four waves: across, up and down, the loop's turn, and the slow breath of the loop's
// size and the nib's pressure. Frequency (rad/s) and phase.
static const float4 FREQ = float4(0.11, 0.17, 1.2, 0.083);
static const float4 PHASE = float4(1.3, 0.0, 0.0, 0.5);

struct Pen
{
    float4 p01;   // (cos, sin) of waves 0 and 1
    float4 p23;   // (cos, sin) of waves 2 and 3
};

Pen penAt(float t)
{
    float4 s, c;
    sincos(FREQ * t + PHASE, s, c);
    Pen p;
    p.p01 = float4(c.x, s.x, c.y, s.y);
    p.p23 = float4(c.z, s.z, c.w, s.w);
    return p;
}

float4 rotBack(float4 p, float4 r)
{
    return float4(p.x * r.x + p.y * r.y, p.y * r.x - p.x * r.y,
                  p.z * r.z + p.w * r.w, p.w * r.z - p.z * r.w);
}

Pen stepBack(Pen p, Pen r)
{
    Pen o;
    o.p01 = rotBack(p.p01, r.p01);
    o.p23 = rotBack(p.p23, r.p23);
    return o;
}

Pen rotation(float dt)
{
    float4 s, c;
    sincos(FREQ * dt, s, c);
    Pen p;
    p.p01 = float4(c.x, s.x, c.y, s.y);
    p.p23 = float4(c.z, s.z, c.w, s.w);
    return p;
}

float2 place(Pen p, float2 amp)
{
    float r = 0.062 * (0.55 + 0.45 * p.p23.z);
    return amp * float2(p.p01.y, p.p01.w) + r * p.p23.xy;
}

float3 inkColour(float age)
{
    float3 hot = float3(1.0, 0.95, 0.84);
    float3 gold = float3(1.0, 0.66, 0.26);
    float3 ember = float3(0.72, 0.16, 0.05);
    float3 c = lerp(hot, gold, saturate(age / 2.5));
    return lerp(c, ember, saturate((age - 2.5) / 10.0));
}

// The nearest point of segment a-b to uv: squared distance and the place along it (0-1).
float2 toSegment(float2 uv, float2 a, float2 b)
{
    float2 seg = b - a;
    float2 q = uv - a;
    float h = saturate(dot(q, seg) / max(dot(seg, seg), 1e-10));
    float2 off = q - seg * h;
    return float2(dot(off, off), h);
}

// One stroke's colour at squared distance d2 from it, `age` seconds after it was written.
// The nib is broad and held at 45 degrees, so strokes across it are wide and strokes
// along it hairlines; the pressure breathes slowly and the stroke thins as it cools.
float3 stroke(float d2, float age, float T, float2 amp, float span, float pix)
{
    Pen p = penAt(T - age);
    float r = 0.062 * (0.55 + 0.45 * p.p23.z);
    float dr = -0.062 * 0.45 * FREQ.w * p.p23.w;
    float2 vel = amp * FREQ.xy * float2(p.p01.x, p.p01.z)
               + r * FREQ.z * float2(-p.p23.y, p.p23.x) + dr * p.p23.xy;
    float2 dir = normalize(vel + 1e-7);
    float broad = abs(dir.x - dir.y) * 0.7071;
    float press = 0.65 + 0.35 * p.p23.w;
    float k = saturate(1.0 - age / span);
    float w = ink * 0.0062 * (0.08 + 0.92 * broad * press) * (0.45 + 0.55 * k) + 0.6 * pix;
    float d = sqrt(d2);
    return inkColour(age) * saturate((w - d) / (2.0 * pix) + 0.5) * (k * k);
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float2 amp = float2(0.5 * aspect - 0.2, 0.31);
    float T = time * pace + 40.0;
    float span = CHUNKS * FINE * STEP;
    float tg = floor(T / STEP) * STEP;
    float lead = T - tg;

    float2 head = place(penAt(T), amp);

    // Coarse pass: the two nearest chunks.
    Pen coarseTurn = rotation(FINE * STEP);
    Pen pen = penAt(tg);
    float2 prev = place(pen, amp);
    float b1 = 1e9, b2 = 1e9;
    float i1 = 0, i2 = 0;
    [unroll]
    for (int c = 0; c < CHUNKS; c++)
    {
        pen = stepBack(pen, coarseTurn);
        float2 cur = place(pen, amp);
        float d2 = toSegment(uv, prev, cur).x;
        bool first = d2 < b1;
        bool second = !first && d2 < b2;
        b2 = first ? b1 : (second ? d2 : b2);
        i2 = first ? i1 : (second ? c : i2);
        b1 = first ? d2 : b1;
        i1 = first ? c : i1;
        prev = cur;
    }

    // Fine pass: the head's own short segment, then both chosen chunks, each drawn on
    // its own so a newer stroke crossing an older one lies cleanly over it.
    float2 hs = toSegment(uv, head, place(penAt(tg), amp));
    float3 col = stroke(hs.x, hs.y * lead, T, amp, span, pix);
    float glow = 0;
    Pen fineTurn = rotation(STEP);
    [unroll]
    for (int k = 0; k < 2; k++)
    {
        float j0 = (k == 0 ? i1 : i2) * FINE;
        Pen p = penAt(tg - j0 * STEP);
        float2 a = place(p, amp);
        float bestD2 = 1e9;
        float bestAge = 0;
        [unroll]
        for (int f = 0; f < FINE; f++)
        {
            p = stepBack(p, fineTurn);
            float2 b = place(p, amp);
            float2 r = toSegment(uv, a, b);
            bool nearer = r.x < bestD2;
            bestD2 = nearer ? r.x : bestD2;
            bestAge = nearer ? lead + (j0 + f + r.y) * STEP : bestAge;
            a = b;
        }
        col = max(col, stroke(bestD2, bestAge, T, amp, span, pix));
    }
    float near = saturate(1.0 - (lead + i1 * FINE * STEP) / span);
    float g = saturate(1.0 - sqrt(b1) / (0.016 * ink)) * near;
    float3 haze = float3(0.9, 0.42, 0.12) * (0.14 * g * g);

    float hd = length(uv - head);
    float3 halo = float3(1.0, 0.85, 0.6) * (0.9 * exp(-hd / 0.0045) + 0.22 * exp(-hd / 0.03));
    return col + haze * (1.0 - col) + halo;
}
