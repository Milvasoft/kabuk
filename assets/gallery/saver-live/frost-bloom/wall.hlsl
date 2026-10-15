// Frost bloom: snow crystals grow out of the dark one after another, six arms at once,
// side branches budding from each arm and twigs from each branch; a glint runs over
// their tips, then each crystal melts from its tips inwards while the next one grows
// somewhere else.
// A crystal is drawn in one twelfth of itself: the pixel is folded into the wedge
// between an arm and the middle of the next gap, so only one arm, the branches nearest
// the pixel and one twig of each need measuring.

static const float SECTOR = 1.0471976;   // 60 degrees
static const float CYCLE = 26.0;          // seconds between two crystals
static const float GROW = 20.0;           // seconds a crystal takes to reach full size
static const float MELT_FROM = 30.0;
static const float MELT_TO = 44.0;
static const float2 BRANCH = float2(0.5, 0.8660254);   // side branches leave at 60 degrees

float hash11(float n)
{
    return frac(sin(n * 127.1) * 43758.5453);
}

// Distance from p to the segment from a along unit dir for len, and where along it.
float2 segment(float2 p, float2 a, float2 dir, float len)
{
    float h = clamp(dot(p - a, dir), 0.0, max(len, 0.0));
    return float2(length(p - a - dir * h), h);
}

// One crystal: (line light, glow light, glint light).
float3 crystal(float2 uv, float cycle, float tau, float aspect, float pix)
{
    float seed = cycle * 7.13;
    float side = fmod(abs(cycle), 2.0) * 2.0 - 1.0;
    float S = 0.25 + 0.11 * hash11(seed + 1.0);                   // arm length on screen
    float2 centre = float2(side * (0.1 + 0.4 * hash11(seed + 2.0)) * max(aspect - 0.7, 0.0),
                           (hash11(seed + 3.0) - 0.5) * 0.25);
    float turn = 6.2831853 * hash11(seed + 5.0) + 0.008 * tau;
    float2 p = (uv - centre) / S;
    float cs = cos(turn), sn = sin(turn);
    p = float2(cs * p.x + sn * p.y, -sn * p.x + cs * p.y);
    float r = length(p);
    float ang = atan2(p.y, p.x);
    ang = abs(ang - SECTOR * round(ang / SECTOR));
    p = r * float2(cos(ang), sin(ang));
    float lp = pix / S;                                         // a pixel in crystal units

    float grown = saturate(tau / GROW);
    float L = 1.0 - (1.0 - grown) * (1.0 - grown);              // fast at first, then slow
    float melt = saturate((tau - MELT_FROM) / (MELT_TO - MELT_FROM));
    float front = 1.15 - 1.3 * melt;                            // melting from the tips in

    float spacing = 0.12 + 0.06 * hash11(seed + 4.0);
    float best = 1e3;                                           // distance to the nearest ice
    float thick = 0.0;                                          // its half width there
    float glint = 0.0;

    // The arm.
    float2 arm = segment(p, float2(0.0, 0.0), float2(1.0, 0.0), L);
    best = arm.x;
    thick = 0.0075 * (1.0 - 0.6 * arm.y);

    // The side branches nearest the pixel, and on each the twig nearest it.
    float i0 = round((p.x - p.y * 0.57735) / spacing);
    [unroll]
    for (int k = -1; k <= 1; k++)
    {
        float i = i0 + k;
        float x = i * spacing;
        float hi = hash11(seed + i * 3.7);
        // Its final length: shorter towards the tip, and never across the gap to the next arm.
        float full = min((1.0 - x) * (0.35 + 0.6 * hi), 0.8 * x);
        float len = clamp((L - x) * 0.85, 0.0, full) * step(1.0, i);
        float2 b = segment(p, float2(x, 0.0), BRANCH, len);
        float bw = 0.0055 * (1.0 - 0.5 * b.y / max(full, 1e-3)) * (1.0 - 0.4 * x);
        bool nearer = b.x < best && len > 0.0;
        thick = nearer ? bw : thick;
        best = nearer ? b.x : best;

        // Twigs leave the branch parallel to the arm, every so often along it.
        float2 q = p - float2(x, 0.0);
        float s2 = spacing * 0.75;
        float j = max(round(q.y / BRANCH.y / s2), 1.0);
        float u = j * s2;
        float hj = hash11(seed + i * 5.1 + j * 1.3);
        float twigFull = max(full - u, 0.0) * (0.2 + 0.35 * hj);
        float twigLen = clamp((len - u) * 0.6, 0.0, twigFull);
        float2 tw = segment(p, float2(x, 0.0) + BRANCH * u, float2(1.0, 0.0), twigLen);
        bool twNearer = tw.x < best && twigLen > 0.0;
        thick = twNearer ? 0.004 : thick;
        best = twNearer ? tw.x : best;

        // A glint that wanders over the branch tips.
        float2 tip = float2(x, 0.0) + BRANCH * len;
        float flash = pow(0.5 + 0.5 * sin(tau * 1.7 + hi * 40.0), 40.0) * step(0.02, len);
        glint += flash * exp(-length(p - tip) * S / 0.004);
    }

    // The hexagonal plate at the heart, with a smaller ridge inside it.
    float plate = min(L * 0.7, 0.16);
    float hexD = dot(p, float2(0.8660254, 0.5)) - plate * 0.8660254;
    float ridge = abs(dot(p, float2(0.8660254, 0.5)) - plate * 0.55 * 0.8660254);
    float rim = min(abs(hexD), ridge);
    bool plateNearer = rim < best;
    thick = plateNearer ? 0.0035 : thick;
    best = min(best, rim);
    float fill = step(hexD, 0.0) * 0.06;

    float alive = smoothstep(front, front - 0.15, r) * step(0.0, tau);
    float core = saturate((thick + 0.6 * lp - best) / lp + 0.5);
    float glow = exp(-best * S / 0.0045);
    return float3(core + fill, glow, glint * sparkle) * alive;
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float t = time * growth + 6.0;
    float cycle = floor(t / CYCLE);
    float tau = t - cycle * CYCLE;
    float3 a = crystal(uv, cycle, tau, aspect, pix);
    float3 b = crystal(uv, cycle - 1.0, tau + CYCLE, aspect, pix);
    float3 hueA = lerp(float3(0.45, 0.75, 1.0), float3(0.7, 0.62, 1.0), hash11(cycle * 7.13 + 9.0));
    float3 hueB = lerp(float3(0.45, 0.75, 1.0), float3(0.7, 0.62, 1.0), hash11((cycle - 1.0) * 7.13 + 9.0));
    float3 white = float3(0.88, 0.95, 1.0);
    float3 col = white * a.x * 0.85 + hueA * a.y * 0.28 + float3(1.0, 1.0, 1.0) * a.z;
    col += white * b.x * 0.85 + hueB * b.y * 0.28 + float3(1.0, 1.0, 1.0) * b.z;
    return col;
}
