// Velvet mesh: five soft pools of colour drifting and melting into each
// other, the field folded gently like velvet catching the light.

static const float VM_TAU = 6.2831853;

float3 vm_hue(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

float vm_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.022;

    // Gentle folds bend the field before the colours are mixed.
    float2 w = uv;
    w += 0.06 * folds * float2(sin(uv.y * 3.1 + t * 1.3), sin(uv.x * 2.3 - t * 1.1));
    w += 0.03 * folds * float2(sin(w.y * 6.7 - t * 0.9), sin(w.x * 5.3 + t * 0.7));

    float3 cols[5] = {
        float3(1.00, 0.42, 0.40),
        float3(0.86, 0.30, 0.66),
        float3(0.50, 0.30, 0.92),
        float3(0.24, 0.44, 0.96),
        float3(1.00, 0.70, 0.52)
    };
    float3 acc = 0.0;
    float wsum = 0.0;
    [unroll] for (int i = 0; i < 5; i++)
    {
        float fi = i;
        float2 c = float2(aspect * (0.25 + 0.6 * (0.5 + 0.5 * sin(t * (0.7 + 0.13 * fi) + fi * 1.9))),
                          0.5 + 0.42 * sin(t * (0.53 + 0.11 * fi) + fi * 2.7));
        float2 d = w - c;
        float dd = dot(d, d);
        float k = 1.0 / (dd * dd + 0.0015);
        acc += cols[i] * k;
        wsum += k;
    }
    float3 col = acc / max(wsum, 1e-6);
    // Mixing dulls colour; win some of it back.
    float grey = dot(col, float3(0.30, 0.59, 0.11));
    col = saturate(lerp(grey.xxx, col, 1.2));
    col = vm_hue(col, hue * VM_TAU);

    // The folds catch light on one side and fall into shade on the other.
    // Broad drapes of cloth: a gentle height field lit like satin.
    float2 da = float2(4.4, 2.2);
    float2 db = float2(-1.6, 4.6);
    float pa = dot(w, da) + 1.2 * sin(w.y * 2.2 - w.x * 0.8 + t * 1.5);
    float pb = dot(w, db) - t * 1.1;
    float2 g = 0.03 * cos(pa) * da + 0.012 * cos(pb) * db;
    float3 n = normalize(float3(-g * 6.0, 1.0));
    float3 l = normalize(float3(-0.5, -0.6, 0.62));
    float diff = dot(n, l) - l.z;
    float spec = pow(saturate(dot(n, normalize(l + float3(0.0, 0.0, 1.0)))), 40.0);
    col *= 1.0 + diff * 0.9 * folds;
    col += float3(1.0, 0.96, 0.94) * spec * 0.22 * folds;
    // Soften towards the icons on the left.
    float side = smoothstep(0.0, 0.35, uv.x / aspect);
    col = lerp(lerp(col, float3(0.86, 0.84, 0.90), 0.35), col, side);

    // A whisper of dither keeps the wide gradients free of bands.
    col += (vm_hash(px) - 0.5) / 255.0;
    return col;
}
