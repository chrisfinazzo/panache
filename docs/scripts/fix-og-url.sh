#!/usr/bin/env bash
set -euo pipefail

# Quarto omits og:url, so reuse each rendered canonical instead of giving all
# pages the homepage's identity when they are shared.
out_dir="${QUARTO_PROJECT_OUTPUT_DIR:-_site}"

[ -d "$out_dir" ] || exit 0

# Perl must expand the captured URL without shell interpolation.
# shellcheck disable=SC2016
find "$out_dir" -name '*.html' -print0 \
  | xargs -0 --no-run-if-empty perl -0pi -e '
      if (m{<link rel="canonical" href="([^"]+)"\s*/?>}) {
        my $canonical = $1;
        s{<meta property="og:url" content="[^"]*"\s*/?>\n?}{}g;
        s{</head>}{<meta property="og:url" content="$canonical">\n</head>};
      }
    '
