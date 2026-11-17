//! Parameter descriptions retained when reorganizing the panels.
pub fn help(name: &str) -> &'static str {
    match name {
        "Refraction Distortion Strength" => {
            "Dimensionless, 0–2, default 1. Scales the UV offset estimated by local planar refraction; 0 disables the offset. Applies live; save manually; no reset."
        }
        "Full Refraction Distortion Depth (m)" => {
            "0.01–20 m, default 2. Gradually reduces the offset below this depth to reduce stretching in thin nearshore water. Applies live; save manually; no reset."
        }
        "Maximum Refraction Offset (% of screen height)" => {
            "0–20%, default 4%. Limits the maximum screen displacement from approximate refraction, normalized by screen height for consistency across aspect ratios. Applies live; save manually; no reset."
        }
        "Enable Underwater Caustics" => {
            "The actual FFT/SWE surface refracts sunlight onto the flat bottom, sand slope, and reef height field. The model uses the nearest intersection with a layered height field; scanned materials receive only focused brightening. Caves and overhangs are unsupported. Off by default; applies live; save manually; does not reset the simulation."
        }
        "Caustics Intensity" => {
            "Dimensionless, 0–4, default 1. Controls transmitted sunlight contribution; 0 restores the original direct light. Applies live; save manually; no reset."
        }
        "Caustics Coverage Width (m)" => {
            "8–512 m, default 128. A fixed world-space square centered on the SWE or model region when enabled, otherwise on the world origin. Edges fade to flat-water transmitted light; does not follow the camera. Applies live; no reset."
        }
        "Caustics Softening (texels)" => {
            "0–4 light-map texels, default 1. Controls spatial filtering of bright lines; the map is fixed at 512×512. Applies live; save manually; no reset."
        }
        "Caustics Beam Grid" => {
            "128/256/512 cells per axis, default 256. Increases beam sampling density to reduce missed samples and bright-line flicker, at greater GPU cost. Applies live; save manually; does not reset water."
        }
        "Enable Underwater Refraction" => {
            "Refracts from air into water; intersects the flat bottom/sand slope analytically and queries the screen depth hierarchy along the refracted direction for models. Falls back to straight-line sampling off-screen, without a visible intersection, or beyond the optical path limit. Off by default; applies live; save manually; no reset."
        }
        "Water Refractive Index" => {
            "Dimensionless, 1–1.6, default 1.333. Controls caustic and view refraction bending, as well as water-surface Fresnel reflection and sun highlights. At 1 the interface has no bending or reflection. Applies live; save manually; no reset."
        }
        "Maximum Refraction Path Length (m)" => {
            "1–500 m, default 100. Limits refracted sampling distance along the view ray; beyond it, falls back to straight-line sampling. Applies live; save manually; no reset."
        }
        "Random Seed" => "Spectrum random seed; click Apply to rebuild the ocean surface.",
        "Parameter Status" => "Spectrum parameters take effect after clicking Apply.",
        "Simulation Time" => "Elapsed simulation time.",
        "Grid Cells per Axis" => {
            "SWE simulation resolution, 64–1024, default 256. Coverage remains unchanged; higher values give finer cells, increase GPU memory and computation time, and shorten the stable time step. Switching immediately rebuilds and reinitializes nearshore water, foam, and wet-sand history; save manually."
        }
        "Grid Spacing X/Z (m)" => {
            "Current simulation extent divided by cells per axis; X points toward shore, Z along shore."
        }
        "Dual-Concentration Experiment" => {
            "Off by default. When enabled, fresh N and residual R share SWE flow for semi-Lagrangian transport and are compressed/diluted together; inertia, drag, and conservative foam flux are no longer used. All sources add only N; R remains only on water. N can use a constant, noise, or NSH texture appearance; R retains its filament appearance. The single-concentration material selection is preserved independently. Switching resets nearshore water, foam, and wet sand; save the configuration manually."
        }
        "Fresh Half-Life (s)" => {
            "Simulation seconds, 0.1–60, default 1.4. Total exit half-life of N, including reduction from conversion to R; decay is not applied twice. Applies live in dual-concentration mode; save manually; history is not cleared."
        }
        "Residual Half-Life (s)" => {
            "Simulation seconds, 0.1–120, default 9. R decays exponentially and independently; newly converted R also decays immediately. Applies live in dual-concentration mode; save manually; history is not cleared."
        }
        "Exit-to-Residual Fraction" => {
            "Dimensionless, 0–1, default 0.65. 65% of exiting N converts to R and the rest disappears directly; 0 converts none, 1 converts all. Local conversion and decay use a coupled analytic solution. Applies live in dual-concentration mode; save manually; history is not cleared."
        }
        "Prototype Hole Appearance" => {
            "Off by default. Samples three-scale noise in transport coordinates, with density controlling hole breakup; overrides the old texture selection without changing generation, transport, drag, or decay. Fixed spatial scales match the HTML prototype; no extra micro normals. Applies live; save manually; no reset."
        }
        "Coordinate Reset Period (s)" => {
            "Simulation time, default 2 seconds; choose 0 or 0.25–60 seconds. Longer periods retain more stretching; 0 means continuous deformation without reset and may distort over time. Resets only invisible coordinate layers, not foam; save manually. Changing the period may change phase blending; reset the simulation for strict comparisons."
        }
        "Flow-Warped Texture" => {
            "On by default. When off, nearshore foam textures use fixed world coordinates for both fresh N and residual R; concentrations still flow, converge, generate, and decay normally. Displacement history continues updating, so re-enabling immediately shows the current distortion. Boolean switch; applies live; save manually; does not reset the simulation."
        }
        "Texture Source" => {
            "0 is the original grayscale effect; the others are linear NSH: normal XY, Soft, Height. All three maps use the same density and dual-phase motion and can be compared while paused. Applies live; save manually; does not reset the simulation; changes only nearshore water foam."
        }
        "Texture Size (m)" => {
            "World-space length covered by each coarse tile, 0.25–64 m, default 12 m; the fine layer is fixed at 1/10 of this. Applies only to the NSH path. Applies live; save manually; does not reset density or displacement history."
        }
        "Foam Micro Normal Strength" => {
            "Dimensionless, 0–3, default 0.65; 0 disables fine NSH normals while retaining density-thickness normals. Changes only foam lighting; applies live; save manually."
        }
        "Opacity Density Scale" => {
            "Simulated foam amount/area, not a measured mass unit; range 0.01–20, default 2. NSH opacity = pore coverage × (1 − exp(−raw density / this value)). At full coverage, density equal to this value gives about 63%, and three times gives about 95%; larger values reveal more water without changing the NSH pore threshold. Dual-concentration constant/noise modes use 1 − exp(−density / this value) for N and R respectively to control white patch and filament coverage; N texture mode multiplies pore coverage by this response, while R is unchanged. Prototype mode uses the same response to control the noise-hole threshold; 1/6 matches the HTML density response. Applies live; save manually; does not reset or change simulated density."
        }
        "Foam Roughness" => {
            "Dimensionless, 0.15–1, default 0.85; higher values broaden highlights. Affects only simplified NSH foam lighting; applies live; save manually."
        }
        "Material Debug" => {
            "Shows pattern, Height, normal, opacity, or a 0.5 m transport-coordinate grid, including dual-phase blending. In dual-concentration mode Soft/Height show the N threshold field or NSH Soft/R noise respectively; the normal view shows N micro normals where its texture covers, and original normals elsewhere. In prototype mode Soft shows noise and Height is zero; the grid is independent of foam density for easier deformation inspection. Enabled when foam is shown in NSH/prototype mode and the Shallow Water view uses normal shading; does not override density debugging. Applies live; save manually."
        }
        "N Rendering Mode" => {
            "Only for dual-concentration fresh foam N: constant uses a uniform threshold, noise uses a two-scale gradient-noise threshold (default), and texture uses NSH pores, micro normals, and roughness lighting. Choose one of three; dimensionless. Texture and original threshold modes have different concentration-to-coverage curves, but both retain zero-concentration and thin-water protection. Switching preserves each mode's parameters; applies live; save manually; no reset. Does not change N/R simulation, R's material, or the single-concentration material."
        }
        "N Texture Source" => {
            "1 Shorewaves NSH, 2 Crest Foam NSH, 3 Crest Foam2 NSH, default 1. Uses only N concentration to render pores and micro normals from three baked linear texture sets, with 72%/28% coarse/fine layers; excludes the original grayscale path. Only effective in N texture mode; updates live; save manually; no reset; does not change R or the single-concentration material."
        }
        "N Texture Size (m)" => {
            "0.25–64 m, default 12. World-space tile edge length of N's coarse texture layer; the fine layer is 1/10 of this. Uses shared transport coordinates and dual-phase blending. Only effective in N texture mode; updates live; save manually; does not reset history or change R."
        }
        "N Micro Normal Strength" => {
            "Dimensionless, 0–3, default 0.65. Controls N's NSH texture micro normals; 0 disables them while retaining the original macro normals. Transforms with transport-coordinate rotation/shear; changes only N lighting, not R normals or geometric height. Only effective in N texture mode; updates live; save manually; no reset."
        }
        "N Roughness" => {
            "Dimensionless, 0.15–1, default 0.85. Roughness for N's simplified NSH lighting; larger values broaden highlights. Does not change water or R lighting. Only effective in N texture mode; updates live; save manually; does not reset the simulation."
        }
        "N Coverage Reference" => {
            "Simulated foam amount/area, not a measured mass unit, 0.01–20, default 2. Controls the range over which N texture pores fill with concentration; larger values expose more water at the same concentration. Opacity is additionally multiplied by 1−exp(−N / shared opacity density scale). Independent of the density debug color scale; does not clamp or change concentration or change R. Only effective in N texture mode; updates live; save manually; no reset."
        }
        "N Constant Threshold" => {
            "Dimensionless, 0–1, default 0.5. Uniform comparison threshold for fresh N coverage; lower values show foam more easily, higher values expose water more easily. Compared against the exponential density response, not raw N concentration. Retains the original transition width; zero concentration remains invisible. Only effective in constant mode; updates live; save manually; does not reset the simulation."
        }
        "N Main Noise Frequency (per m)" => {
            "0.05–20/m, default 0.8. Controls the spatial frequency of N's main noise; larger values produce finer patterns, not an exact periodic wavelength. Uses shared transport coordinates, retaining the original triangular-grid gradient noise and footprint filtering. Only effective in noise mode; updates live; save manually; no reset; does not change R or simulated concentration."
        }
        "N Fine Noise Frequency (per m)" => {
            "0.05–60/m, default 2.5. Controls the spatial frequency of N's detail layer; larger values produce finer patterns. The fine layer retains its original rotation and offset; finer details may be filtered by the pixel footprint. Affects appearance only in noise mode with a nonzero fine-layer fraction; updates live; save manually; does not reset the simulation."
        }
        "N Fine Noise Fraction" => {
            "Dimensionless, 0–1, default 0.3. Threshold field = (1−fraction)×main noise + fraction×fine noise; 0 uses only the main layer, 1 only the fine layer; the default retains the original 70%/30%. Only effective in noise mode; updates live; save manually; no reset; does not change R material or simulated concentration."
        }
        "Concentration Controls Opacity" => {
            "Off by default, independent for N and R. When enabled, pow(clamp(own concentration / concentration range,0,1),p) is an additional factor multiplying both the foam's premultiplied color and opacity, without changing the original pore pattern or concentration simulation. Available for all three N rendering modes; R does not change N. Boolean switch; applies live; save manually; no reset. Disabling preserves parameters and restores the original appearance."
        }
        "N Concentration Range" | "R Concentration Range" => {
            "Simulated foam amount/area, not a measured mass unit, 0–10, default 1. This foam's concentration is divided by this value, clamped to 0–1, then raised to power p as an additional opacity factor. Larger values are more transparent at the same concentration; at 0, positive concentration is treated as 1 and zero concentration as 0 to avoid division by zero. Does not clamp simulated concentration. Effective only when the corresponding 'Concentration Controls Opacity' is enabled; updates live; save manually; no reset."
        }
        "N Concentration Exponent p" | "R Concentration Exponent p" => {
            "Dimensionless, 0.1–5, default 1. Raises normalized concentration to power p; p>1 is more transparent, p<1 more opaque. Does not change original pore coverage, the other foam type, or simulated concentration. Effective only when the corresponding 'Concentration Controls Opacity' is enabled; updates live; save manually; no reset."
        }
        "Foam Display Components" => {
            "In dual-concentration mode, choose all, fresh N only, or residual R only; default all. Filters only the nearshore material and foam appearance height; offshore foam is hidden when viewing one component. Dimensionless option; applies live; save manually; does not change generation, transport, conversion, or decay and does not reset the simulation. Density debug views remain independent."
        }
        "Enable Foam Emission" => {
            "Enable/disable new foam emission, enabled by default. Disabling does not clear existing density, momentum, or wet sand; transport, drag, and decay continue. Applies live; save manually; does not reset the simulation."
        }
        "Compression Ridge Experiment" => {
            "Off by default. When enabled, emits only in a narrow band near local compression peaks along the current flow direction, forcibly disabling the background source; no pointwise maximum filtering along the wavefront tangent. Changes only new foam, not hydrodynamics. Applies live; save manually; existing foam is not cleared; start comparisons after a reset."
        }
        "Narrow Band Width (m)" => {
            "0.05–10 m, default 1. The local five-point fit has limited effective range, so actual width is limited to one or two cells along the flow and cannot produce sub-grid thin lines. Effective only in the ridge experiment; save manually; no reset."
        }
        "Peak Relative Prominence" => {
            "0–1, dimensionless, default 0.02. The second difference of central compression above both sides must exceed this fraction; increasing it rejects gentle broad peaks. Uniform compression slopes do not emit. Effective only in the ridge experiment; save manually; no reset."
        }
        "Front Generation Rate (per s)" => {
            "Foam generation rate when directional compression reaches full weight, range 0–30. 0 disables front generation; independent of the background rise threshold. Applies live; save manually; history is not cleared."
        }
        "Front Sampling Distance (m)" => {
            "Effective only for the old front source; the ridge experiment always reads adjacent cells. Uniformly samples 4 positions within this distance along the current flow direction, range 0.05–10 m, default 1 m. The approaching velocity difference along the sampling direction is divided by actual distance, then the maximum compression strength is taken; same-direction chasing can trigger it, while separation and purely transverse velocity differences cannot. Samples by world distance, supporting non-square cells; dry and out-of-bounds points are skipped. Applies live; history is not cleared."
        }
        "Velocity Change Start Threshold (per s)" => {
            "Foam generation starts only when the compressive velocity difference along the sampling direction divided by the actual sampling distance exceeds this value, range 0–20, default 0.1. For example, a velocity difference of 1 m/s over 0.5 m gives strength 2/s. Only approaching components count. Applies live; save manually; history is not cleared."
        }
        "Velocity Change Transition Range (per s)" => {
            "Range 0–20, default 0.5; generation weight smoothly rises to 1 as strength goes from the start threshold to the threshold plus this range. At 0, generation requires strictly exceeding the threshold; zero velocity difference produces no foam. Applies live; save manually; history is not cleared."
        }
        "Background Generation Multiplier" => {
            "Range 0–1, default 0.15; scales only background sources triggered by rising water, flow speed, and convergence. Set to 0 to observe the old front alone; the ridge experiment forces background to zero without changing this saved value. Applies live; save manually; history is not cleared."
        }
        "Foam Spatial Transport" => {
            "Dimensionless; 1 is the original first-order flux (default), 2 is limited MUSCL reconstruction, reducing numerical diffusion of filaments and holes. Density and momentum are transported conservatively together; density is not capped at 1. This selection applies only to second-order SWE (RK2); first-order SWE always uses the original flux. Changes apply live; save manually; history is not cleared; generation, decay, and material are unchanged."
        }
        "Enable Flow Drag" => {
            "When enabled, flow gradually changes foam momentum; when disabled, inertial transport remains, useful for isolating chasing convergence. Generation still injects momentum, decay still removes it, and motion stops on sand. Applies live; save manually; no reset."
        }
        "Flow Following Multiplier" => {
            "Drag target velocity = flow velocity × this multiplier, range 0–1. Effective only when drag is enabled. The physical reference is 1; below 1 adds visual resistance. Foam amount and momentum are transported together, merging velocity is weighted by foam amount, and water receives no reaction force. Applies live; save manually."
        }
        "Velocity Response Time (s)" => {
            "Range 0–20; larger values retain more inertia, 0 follows flow immediately. Approximately 63% of the velocity response completes after this time; effective only when drag is enabled. The physical reference starting point is 2 seconds, a simulation calibration value. Applies live; save manually; history is not cleared."
        }
        "New Foam Velocity Multiplier" => {
            "New foam momentum = added amount × local flow velocity × this multiplier, range 0–1, default and physical reference 1. Affects only new foam; existing foam retains momentum. Still effective with drag disabled. Applies live; save manually; no reset."
        }
        "Physical Reference Starting Point" => {
            "Apply: drag enabled, following multiplier 1, response time 2 seconds, new foam velocity multiplier 1, water foam decay 0.3/s. A calibration starting point based on linear drag and foam decay on the scale of seconds, not a universal measured seawater constant. Does not change generation, sand, or display parameters; applies live; save manually."
        }
        "Maximum Height (m)" => {
            "Range 0–1 m, default 0.05 m; 0 disables the height effect. Moves render vertices upward based only on nearshore water foam density; does not increase water level, depth, or momentum. Foam shading uses the raised normals; water still uses the original surface. Applies live; save manually; history is not reset."
        }
        "Starting Density" => {
            "Range 0–20, default 0.1. Uses simulated foam amount/area, not clamped to 0–1; no raising at or below this value. Independent of the density color reference and does not sample pore textures. Applies only to nearshore water foam; offshore foam is not raised. Applies live; save manually."
        }
        "Density Transition Range" => {
            "Range 0.001–20, default 1.9. As density rises from the starting value to that value plus this range, smoothstep smoothly increases height from 0 to the maximum; above that it stays at the maximum. Smoothly weakens below 20 mm depth, with no raising at 8 mm or less; hiding foam also disables height. Applies live; save manually."
        }
        "Water Foam Decay (per s)" => {
            "Used only by the old single-concentration path, range 0–4/s; dual-concentration mode uses the two half-lives instead, without adding this decay. Applies live; save manually; history is not reset."
        }
        _ => legacy_help(name),
    }
}

fn legacy_help(name: &str) -> &'static str {
    match name {
        "Spectrum Strength A" => {
            "Phillips spectrum strength; wave height is proportional to the square root of A. Click Apply to rebuild the spectrum."
        }
        "Spectrum Scale L (m)" => {
            "Phillips length scale, equal to wind speed squared divided by gravitational acceleration. Click Apply to take effect."
        }
        "Wind Direction (degrees)" => {
            "Direction of the wind unit vector, not wave speed. Click Apply to rebuild the spectrum."
        }
        "Patch Size (m)" => {
            "Edge length of a periodic FFT patch; the ocean consists of 3×3 patches. Click Apply to take effect."
        }
        "FFT Resolution" => {
            "FFT samples per axis; sample spacing equals patch size divided by resolution. Click Apply to take effect."
        }
        "Offshore Grid Density" => {
            "Dimensionless, 8–512, default 128. In square-grid mode this is the cell count per axis; near-equilateral triangle mode adjusts rows and columns to a similar triangle count. When only density changes, clicking Apply rebuilds only the offshore draw mesh without resetting the simulation; changing it with the spectrum still rebuilds the simulation."
        }
        "Near-Equilateral Triangle Grid" => {
            "Off by default. When enabled, offshore draw vertices in alternating rows are offset by half a cell to form periodically tiled near-equilateral triangles; the triangle count remains nearly unchanged. Applies immediately and can be compared while paused; does not reset FFT, SWE, or foam history or apply other spectrum drafts. The local SWE surface still uses a square grid; save manually."
        }
        "Pause" => {
            "Freeze simulation, foam, and texture flow while allowing rendering parameters to be adjusted."
        }
        "Time Scale" => "Simulation time speed multiplier; 0 stops evolution.",
        "Wave Height Gain" => {
            "Scales both FFT displacement and SWE input; changes reinitialize Shallow Water and foam history."
        }
        "Horizontal Choppiness" | "Choppiness Strength" => {
            "FFT horizontal displacement sharpens wave crests and changes the Jacobian used to generate offshore foam."
        }
        "Show Foam" => "Only toggles foam display; does not stop simulation.",
        "Jacobian Threshold" => {
            "Offshore foam is generated when the surface deformation Jacobian falls below this threshold. Applies live."
        }
        "Generation Rate (per s)" => {
            "Offshore foam generation rate per simulation second; applies live."
        }
        "Decay Coefficient (per s)" => {
            "Exponential decay coefficient for offshore foam; smaller values last longer, 0 means no natural decay."
        }
        "Foam View" => {
            "Choose normal water surface, deformation Jacobian, or offshore foam coverage."
        }
        "Sloping Beach" => {
            "Enable the sloping beach: negative X is land, positive X is deep water. Terrain changes reset the Shallow Water simulation."
        }
        "Enable Shallow Water Simulation" => {
            "Enable local Shallow Water simulation and foam and sand history. Switching resets SWE."
        }
        "SWE Solver" => {
            "First order uses Euler integration; second order uses MUSCL and RK2. Switching resets the simulation; set normal detail to 0 when comparing geometry."
        }
        "Water Foam" => {
            "Linear RGB color of water foam, including offshore foam. Changes display only."
        }
        "Normal Detail Strength" => {
            "Short-wave normal detail from FFT; 0 is off, 1 is default. Gradually weakens in thin water; does not reset the simulation. Easier to see with reflections enabled."
        }
        "FFT → SWE Gain" => {
            "Scales both input water level and momentum: 0 is no input, 1 is matched, 3 is triple. Changes initialize the entire Shallow Water region from the current FFT and clear history."
        }
        "Filter Short Waves" => {
            "Applies the same short-wave filter to FFT water level and momentum input to SWE; offshore detail is unchanged. Changes reinitialize Shallow Water."
        }
        "Cutoff Wavelength (m)" => {
            "Waves at or below this wavelength are filtered; full transmission begins at the cutoff wavelength plus the transition width."
        }
        "Filter Transition Width (m)" => {
            "Smooth filtering range above the cutoff wavelength; 0 is a hard switch. Changes reinitialize Shallow Water."
        }
        "FFT Blend Depth (m)" => {
            "Transitions from SWE to FFT between this depth and 2.5 times this depth. Changes reset Shallow Water."
        }
        "Background Generation Rate (per s)" => {
            "Range 0–15, default 4; units are foam area density added per simulation second, additionally multiplied by the background generation multiplier. 0 disables the background source; the front rate is independent. Applies live; save manually; history is not reset."
        }
        "Background Generation Start Threshold" => {
            "Background foam generation begins when water depth rise rate divided by shallow-water wave speed exceeds this value. Dimensionless; lower values generate more easily; default 0.25. Applies live; no reset."
        }
        "Background Generation Transition Range" => {
            "Transition width from the start threshold to full generation weight. Default 0.4, meaning starts at 0.25 and reaches full weight at 0.65. 0 is a hard threshold; does not affect front generation."
        }
        "Water Foam Decay (per s)" => {
            "Exponential decay coefficient for water foam; smaller values last longer, 0 means no natural decay."
        }
        "Convergence Enhancement Threshold (per s)" => {
            "Flow convergence threshold for enhancing background foam generation. Existing foam still accumulates naturally through transport, regardless of this threshold."
        }
        "Density Color Reference" => {
            "Simulated foam amount/area, range 0.01–20, default 2. At this value the density debug display is red; also controls the material pore filling range, without limiting simulated density. NSH intensity is separately controlled by opacity density scale. Applies live; save manually; no reset."
        }
        "Sand Drying Time (s)" => "Wet-sand drying time constant; does not control foam lifetime.",
        "Shallow Water Debug View" => {
            "Debug views skip normal shading. Flow direction: blue toward shore, orange receding. Density: low green, high red, without pore texture blending."
        }
        "Sand Texture Rotation (degrees)" => {
            "Rotates the texture in the sand-slope plane around the world origin. Range -180–180 degrees, default 0 degrees; a positive angle turns the texture's horizontal axis from downslope +X toward alongshore +Z. All sand maps and normal directions rotate together; applies live and saves with the configuration; does not reset the simulation."
        }
        "Sand Texture Size (m)" => {
            "Length covered by each sand texture tile along the slope and shore. Range 0.25–128 m; larger values make larger sand patterns with less repetition. Color, normals, roughness, and AO scale together; applies live and saves with the configuration; does not reset the simulation."
        }
        "Beach Slope (height/width)" | "Shoreline X (m)" => {
            "Negative X is land, positive X is deep water. Changes reinitialize the Shallow Water simulation."
        }
        "Seabed Depth (m)" => {
            "In meters, 0.25–100. Controls the seabed in the flat scene and the supplemental seabed in the model scene; changes rebuild immediately and reset nearshore history. Disabled with a purely sloping beach."
        }
        "Enable Sky Scattering" => {
            "Isotropic sky scattering based on mean sea level and straight light paths. The manual light source uses analytic integration; the sky source uses piecewise analytic integration."
        }
        "Sky Color (linear RGB)" => {
            "Linear RGB color of sky illumination, affecting both background and reflections."
        }
        "Sky Intensity" => "Radiance multiplier for sky illumination.",
        "Scattering Albedo RGB" => {
            "Scattering coefficient = albedo × extinction coefficient; absorption coefficient = (1 − albedo) × extinction coefficient."
        }
        "Sky Integration Steps" => {
            "Number of integration segments for sky scattering; more is more accurate but costs more GPU time."
        }
        "Water Debug View" => {
            "Optical debugging skips reflections and foam. Transmittance decays exponentially with extinction coefficient and path length; absorption view shows the attenuated background, with losses including out-scattering."
        }
        "Path Length White Reference (m)" => {
            "Distance displayed as white in path-length debugging."
        }
        "Sky Reflection and Highlights" => {
            "Controls water surface reflection and sun highlights, as well as screen-space reflection below. Applies live."
        }
        "Screen-Space Reflection (SSR)" => {
            "Queries camera depth along the wave-surface reflection direction, reflecting above-water models and shore visible on screen. Uses sky reflection on a miss. Applies live."
        }
        "SSR Strength" => {
            "Blend weight between valid SSR hits and sky reflection, dimensionless, 0–1. 0 uses sky reflection and stops SSR tracing; 1 uses full SSR. Applies live."
        }
        "SSR Trace Steps" => {
            "Maximum Hi-Z tiles visited per reflection ray, 16–256, default 128. Lower values limit cost but may miss reflections and fall back to sky. Applies live."
        }
        "SSR Hit Tolerance" => {
            "Dimensionless multiplier for hit tolerance, 0.25–4, default 1. The baseline varies with the pixel's world-space footprint. Smaller values may miss hits; larger values may produce false hits. Applies live."
        }
        "SSR Edge Fade (pixels)" => {
            "Distance from the render image edge at which samples begin fading to sky, 0–128 render pixels, default 4. 0 disables fading; out-of-bounds samples still fall back to sky. Applies live."
        }
        "Reflection Distance (m)" => {
            "Maximum screen-space reflection ray length, 1–500 m. Fades to sky reflection near the limit. Applies live."
        }
        "Seabed RGB Test Bands" | "Checker Size (m)" => {
            "Seabed calibration pattern for comparing absorption of different colors and optical path lengths."
        }
        _ => "Live parameter; changes take effect immediately.",
    }
}
