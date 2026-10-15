// Iznik glaze: a wall of Ottoman tiles in cobalt, turquoise and coral on a
// white glaze. The rosette at the heart of each tile turns very slowly, the
// half-rosettes where four tiles meet turn the other way, and a soft window
// of light slides across the glaze, catching the rounded edge of every tile.
// Towards the icons on the left the tiles fall quiet.

static const float IZ_TAU = 6.2831853;

float iz_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float2 iz_rot(float2 p, float a)
{
    float c = cos(a);
    float s = sin(a);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

// A rosette of n petals between radii r0 (the valleys) and r1 (the tips):
// a radial distance, negative inside. `pt` shapes the petal (small: round).
float iz_petals(float2 p, float n, float r0, float r1, float pt, float shift)
{
    float r = length(p);
    float a = atan2(p.y, p.x);
    float k = frac(a * n / IZ_TAU + shift) - 0.5;
    float edge = r0 + (r1 - r0) * pow(saturate(1.0 - abs(2.0 * k)), pt);
    return r - edge;
}

// Lays a shape of colour `fill` over `col`, with pigment pooling at its edge
// and a fine dark outline, the way the painters drew them.
void iz_paint(inout float3 col, float d, float3 fill, float aa, float strength)
{
    float inside = 1.0 - smoothstep(-aa, aa, d);
    float pool = lerp(0.78, 1.0, smoothstep(0.0, 0.03, -d));
    col = lerp(col, fill * pool, inside * strength);
    float ink = 0.0045;
    float ln = 1.0 - smoothstep(ink - aa, ink + aa, abs(d));
    col = lerp(col, float3(0.07, 0.09, 0.16), ln * 0.8 * strength);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;

    float tile = 0.34 / scale;
    float2 g = uv / tile;
    float2 cell = floor(g);
    float2 q = g - cell - 0.5;
    float aa = 1.0 / (resolution.y * tile);
    float h = iz_hash(cell + 7.0);

    // Ground and pigments; the "ground" value turns the wall inside out:
    // white figures on a cobalt glaze.
    float3 white = float3(0.90, 0.88, 0.82);
    float3 cobalt = float3(0.08, 0.18, 0.50);
    float3 bg = lerp(white, cobalt * 0.9, ground);
    float3 blueC = lerp(cobalt, white, ground);
    float3 turq = float3(0.15, 0.54, 0.57);
    float3 coral = float3(0.78, 0.24, 0.17);
    float3 green = float3(0.20, 0.46, 0.31);

    // The left column of tiles is left plain for the icons: whole tiles, so
    // the panel ends like a real one, with the next column's half-flowers.
    float2 sq = step(0.0, q) * 2.0 - 1.0;
    float edgeX = aspect * 0.12;
    float side = step(edgeX, (cell.x + 0.5) * tile);

    float3 col = bg;
    // A faint stain of age in the glaze, different in every tile.
    col *= 0.97 + 0.05 * h;

    // Saz leaves along the diagonals, from the rosette out to the corners.
    float2 dv = normalize(sq);
    float lx = dot(q, dv);
    float ly = dot(q, float2(-dv.y, dv.x));
    float lu = saturate((lx - 0.27) / 0.27);
    float leaf = abs(ly) - 0.052 * pow(sin(lu * 3.14159), 0.75);
    leaf = max(leaf, max(0.27 - lx, lx - 0.54));
    iz_paint(col, leaf, lerp(green, turq, 0.35), aa, side);
    // The leaf's vein.
    float vein = abs(ly) - 0.002;
    vein = max(vein, max(0.30 - lx, lx - 0.50));
    col = lerp(col, float3(0.07, 0.09, 0.16), (1.0 - smoothstep(-aa, aa, vein)) * 0.6 * side);

    // Half-rosettes at the corners: one coral flower shared by four tiles.
    float2 corner = cell + 0.5 + 0.5 * sq;
    float hc = iz_hash(corner + 3.0);
    float cside = step(edgeX, (corner.x + 0.5) * tile);
    float2 cq = q - 0.5 * sq;
    cq = iz_rot(cq, -t * 0.009 + hc * IZ_TAU);
    iz_paint(col, iz_petals(cq, 6.0, 0.07, 0.15, 0.55, 0.0), coral, aa, cside);
    iz_paint(col, length(cq) - 0.045, blueC, aa, cside);
    iz_paint(col, length(cq) - 0.016, white, aa, cside);

    // Small turquoise buds where two tiles meet along an edge.
    float2 aq0 = abs(q);
    float2 mid = aq0.x > aq0.y ? float2(0.5 * sq.x, 0.0) : float2(0.0, 0.5 * sq.y);
    float mside = step(edgeX, (cell.x + 0.5 + 2.0 * max(mid.x, 0.0)) * tile);
    float2 mq = q - mid;
    iz_paint(col, iz_petals(mq, 4.0, 0.045, 0.10, 0.6, 0.0), turq, aa, mside);
    iz_paint(col, length(mq) - 0.026, blueC, aa, mside);

    // The central rosette.
    float2 p = iz_rot(q, t * 0.012 + h * IZ_TAU);
    float r = length(p);
    // A ring of twelve beads.
    float a = atan2(p.y, p.x);
    float kb = frac(a * 12.0 / IZ_TAU) - 0.5;
    float bead = length(float2(r - 0.235, r * kb * IZ_TAU / 12.0)) - 0.017;
    iz_paint(col, bead, coral, aa, side);
    iz_paint(col, iz_petals(p, 8.0, 0.085, 0.195, 0.65, 0.0), blueC, aa, side);
    iz_paint(col, iz_petals(p, 8.0, 0.045, 0.125, 0.8, 0.5), turq, aa, side);
    iz_paint(col, r - 0.044, coral, aa, side);
    iz_paint(col, r - 0.015, white, aa, side);

    // Grout between the tiles.
    float2 aq = abs(q);
    float e = max(aq.x, aq.y);
    float grout = smoothstep(0.492 - aa, 0.492 + aa, e);
    float3 groutC = lerp(float3(0.62, 0.60, 0.56), float3(0.05, 0.08, 0.17), ground);
    col = lerp(col, groutC, grout);

    // The glaze: each tile is a slightly domed, slightly tilted pillow.
    float2 nrm = float2(pow(saturate(aq.x * 2.0), 14.0) * sign(q.x), pow(saturate(aq.y * 2.0), 14.0) * sign(q.y)) * 0.7;
    nrm += (float2(h, iz_hash(cell + 19.0)) - 0.5) * 0.06;
    // A broad window of light slides slowly across the wall and back.
    float lp = -0.2 + (aspect * 0.62 + 0.8) * (0.5 + 0.5 * sin(t * 0.011 - 1.2));
    float s = uv.x * 0.62 + uv.y * 0.38 + dot(nrm, float2(0.55, 0.35)) - lp;
    float band = smoothstep(-0.16, 0.0, s) * (1.0 - smoothstep(0.02, 0.30, s));
    // The rounded edges catch a cooler light from above.
    float rim = saturate(dot(nrm, float2(-0.35, -0.75)));
    float lit = band * (0.10 + 0.05 * ground) + rim * 0.08;
    col += float3(1.0, 0.98, 0.94) * lit * sheen * (1.0 - grout) * lerp(0.5, 1.0, side);
    col *= 1.0 - 0.18 * saturate(dot(nrm, float2(0.35, 0.75))) * (1.0 - grout);

    // A gentle falloff to the corners of the screen.
    float2 v = px / resolution - 0.5;
    col *= 1.0 - 0.18 * dot(v, v);
    col += (iz_hash(px) - 0.5) / 255.0;
    return col;
}
