// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

// Filters the register table as the user types. Without this script the whole table shows.
"use strict";

const search = document.getElementById("register-search");
const shown = document.getElementById("register-shown");
const none = document.getElementById("register-none");
const rows = Array.from(document.querySelectorAll("#register tbody tr"));

search.parentElement.hidden = false;
search.addEventListener("input", () => {
  const words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
  let count = 0;
  for (const row of rows) {
    const text = row.textContent.toLowerCase();
    row.hidden = !words.every((word) => text.includes(word));
    if (!row.hidden) count += 1;
  }
  shown.textContent = words.length
    ? `${count} of ${rows.length} businesses shown`
    : `${rows.length} businesses`;
  none.hidden = count > 0;
});
