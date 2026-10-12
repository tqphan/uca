// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use smol_str::SmolStr;

/// An input source, e.g. `"mouse"`, `"keyboard"`, or a plugin id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub SmolStr);

/// A fully qualified item id: `"board/layer/item"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub SmolStr);
