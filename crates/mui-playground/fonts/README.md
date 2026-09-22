# Material Symbols subset

`MaterialSymbolsOutlined-ui.ttf` is the 52-icon subset the playground's
`icon("name")` draws from, cut from Material Symbols Outlined (variable:
FILL, GRAD, opsz, wght; Apache-2.0, google/material-design-icons) with

    python3 -m fontTools.subset MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].ttf \
        --unicodes=<codepoints> --layout-features='*' --no-hinting

`--layout-features='*'` keeps the GSUB FeatureVariations that swap in the
dedicated filled glyph at FILL 1. Names, in codepoint order of the
`.codepoints` file: add arrow_back arrow_forward bookmark check chevron_left chevron_right close content_copy dark_mode delete download edit error expand_less expand_more favorite folder help home info light_mode lock lock_open mail menu mic more_horiz more_vert notifications pause person play_arrow redo refresh remove save search settings share skip_next skip_previous star stop tune undo upload visibility visibility_off volume_off volume_up warning.
