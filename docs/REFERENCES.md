# References

Sources for the scoring targets: peer-reviewed studies on bedroom air and sleep,
plus public indoor-air guidance. The system scores the **room**, not the person,
and makes no health claims.

The agent's `get_targets` tool returns the author-year labels below for each
metric, so it can say where a target comes from.

## Targets and where they come from

| Metric | Our target (100 points) | Sources |
| --- | --- | --- |
| eCO₂ | ≤ 800 ppm; 0 points at ≥ 2,000 ppm | [1], [2], [5]: better bedroom ventilation (lower CO₂) improves sleep quality. [3]: an average of ~1,000 ppm already measurably worsens sleep. |
| Temperature | 65–70 °F | [2], [4]: a warm bedroom disturbs sleep. The exact 65–70 °F band is our choice, informed by these studies; they do not prescribe it. |
| Humidity | 40–60% RH | [6]: adverse effects are minimized between 40 and 60%. [7]: keep indoor humidity below 60% (ideally 30–50%). [4]: high humidity disturbs sleep. |

Caveats:
- The CCS811 **estimates** CO₂ from VOCs (eCO₂). The studies measured real CO₂, so
  their thresholds are a guide for our estimated values, not a direct match.
- We have read the titles and abstracts, not every full paper.

## Sources

1. Fan, X., Liao, C., Bivolarova, M. P., Sekhar, C., Laverge, J., Lan, L., Mainka, A.,
   Akimoto, M., & Wargocki, P. (2022). A field intervention study of the effects of window
   and door opening on bedroom IAQ, sleep quality, and next-day cognitive performance.
   *Building and Environment, 225*, 109630.
   https://doi.org/10.1016/j.buildenv.2022.109630
   — Label: "Fan et al., 2022 (window/door opening)"
2. Fan, X., Shao, H., Sakamoto, M., Kuga, K., Lan, L., Wyon, D. P., Ito, K., Bivolarova,
   M. P., Liao, C., & Wargocki, P. (2022). The effects of ventilation and temperature on
   sleep quality and next-day work performance: Pilot measurements in a climate chamber.
   *Building and Environment, 209*, 108666.
   https://doi.org/10.1016/j.buildenv.2021.108666
   — Label: "Fan et al., 2022 (ventilation and temperature)"
3. Kang, M., Yan, Y., Guo, C., Liu, Y., Fan, X., Wargocki, P., & Lan, L. (2024).
   Ventilation causing an average CO2 concentration of 1,000 ppm negatively affects sleep:
   A field-lab study on healthy young people. *Building and Environment, 249*, 111118.
   https://doi.org/10.1016/j.buildenv.2023.111118
   — Label: "Kang et al., 2024"
4. Okamoto-Mizuno, K., Mizuno, K., Michie, S., Maeda, A., & Iizuka, S. (1999). Effects of
   humid heat exposure on human sleep stages and body temperature. *Sleep, 22*(6), 767–773.
   — Label: "Okamoto-Mizuno et al., 1999"
5. Yan, Y., Kang, M., Zhang, H., Lian, Z., Fan, X., Sekhar, C., Wargocki, P., & Lan, L.
   (2024). Does window/door opening behaviour during summer affect the bedroom environment
   and sleep quality in a high-density sub-tropical city. *Building and Environment, 247*,
   111024. https://doi.org/10.1016/j.buildenv.2023.111024
   — Label: "Yan et al., 2024"
6. Arundel, A. V., Sterling, E. M., Biggin, J. H., & Sterling, T. D. (1986). Indirect health
   effects of relative humidity in indoor environments. *Environmental Health Perspectives,
   65*, 351–361. https://doi.org/10.1289/ehp.8665351
   — Label: "Arundel et al., 1986"
7. U.S. Environmental Protection Agency. A Brief Guide to Mold, Moisture and Your Home.
   https://www.epa.gov/mold/brief-guide-mold-moisture-and-your-home
   — Label: "US EPA mold and moisture guide"
