//! What may cross to another thread (spec §22.2): `Send` and `Share`, worked out from a
//! type's fields as copyability is, with `std`'s hand-written exceptions.

use dz_hir::ty::Ty;
use dz_resolve::SymbolId;

use crate::{ItemSig, TypeTable};

thread_local! {
    /// The types a `Send` (index 0) or `Share` (index 1) question is already working out,
    /// while it asks an implementation's bounds (see `crosses_from_here`).
    static CROSS_STACK: std::cell::RefCell<[Vec<SymbolId>; 2]> =
        const { std::cell::RefCell::new([Vec::new(), Vec::new()]) };
}

/// The two questions §22.2 asks of a type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Crossing {
    /// May move to another thread, or be lent to it as `&mut`.
    Send,
    /// May be read through `&` by several threads at once.
    Share,
}

impl crate::Ctx<'_> {
    /// Whether `ty` reaches an `Rc` or a `Weak` through what it holds, behind references
    /// too, since reading one through `&` writes its count (spec §22.4).
    pub(crate) fn holds_rc(&self, ty: &Ty) -> bool {
        let rc = |sym: SymbolId| {
            let s = self.res.symbol(sym);
            s.module == self.res.prelude && matches!(s.name.as_str(), "Rc" | "Weak")
        };
        self.table.holds_named(ty, &rc, &mut Vec::new())
    }

    /// A [`Blocker`] as a help says it: "its field `m_header` is a raw pointer".
    pub(crate) fn describe_blocker(&self, b: &Blocker) -> String {
        let what = match &b.ty {
            Ty::Ptr { .. } | Ty::RawCStr => {
                "a raw pointer, which may point at anyone's memory".to_owned()
            }
            Ty::Ref { mutable: true, .. } | Ty::Slice { mutable: true, .. } => {
                "a `&mut`".to_owned()
            }
            Ty::Ref { .. } | Ty::Slice { .. } => "a view".to_owned(),
            Ty::Fn { .. } => "a `#[main_thread] fn`".to_owned(),
            Ty::Param(_) | Ty::Assoc(_) => {
                format!("`{}`, whose bounds do not say so", self.name_of(&b.ty))
            }
            Ty::Named { .. } if self.table.dies_on_main_thread(&b.ty) => {
                format!("a `{}`, whose drop runs on the main thread only", self.name_of(&b.ty))
            }
            t => format!("a `{}`", self.name_of(t)),
        };
        if b.path.is_empty() {
            what
        } else {
            format!("its field `{}` holds {what}", b.path.join("."))
        }
    }
}

/// What keeps a type from crossing: the field path to it, empty for the type itself, and its
/// type.
pub struct Blocker {
    pub path: Vec<String>,
    pub ty: Ty,
}

impl TypeTable {
    /// The prelude's `Send` trait, if the prelude declares it.
    pub fn send_trait(&self) -> Option<SymbolId> {
        self.send_trait
    }

    /// The prelude's `Share` trait, if the prelude declares it.
    pub fn share_trait(&self) -> Option<SymbolId> {
        self.share_trait
    }

    /// Whether each thread has its own `sym`, a `#[thread_local]` static (spec §7.7).
    pub fn is_thread_local(&self, sym: SymbolId) -> bool {
        self.thread_locals.contains(&sym)
    }

    /// Whether a value of `ty` runs a `#[main_thread]` drop when it dies: its own type's, or
    /// one of something it holds by value (spec §22.5). Such a value is never `Send` or
    /// `Share`, and a function that makes one or takes one by value is the main thread's,
    /// so it only ever dies there. Through a reference or a raw pointer, nothing dies.
    pub fn dies_on_main_thread(&self, ty: &Ty) -> bool {
        self.dies_on_main_thread_guarded(ty, &mut Vec::new())
    }

    fn dies_on_main_thread_guarded(&self, ty: &Ty, stack: &mut Vec<SymbolId>) -> bool {
        match ty {
            Ty::OwnPtr { inner, .. }
            | Ty::Optional(inner)
            | Ty::Fallible(inner)
            | Ty::Array { elem: inner, .. } => self.dies_on_main_thread_guarded(inner, stack),
            Ty::Tuple(parts) => parts.iter().any(|t| self.dies_on_main_thread_guarded(t, stack)),
            Ty::Named { sym, args } => {
                if stack.contains(sym) {
                    return false;
                }
                if self.own_drop_on_main_thread(*sym) {
                    return true;
                }
                stack.push(*sym);
                let answer = match self.get(*sym) {
                    Some(ItemSig::Struct(_)) => self.with_bases(*sym).into_iter().any(|owner| {
                        let Some(s) = self.struct_sig(owner) else { return false };
                        let own_args = self.args_as(*sym, args, owner);
                        self.field_types(owner, &own_args, &s.fields)
                            .iter()
                            .any(|t| self.dies_on_main_thread_guarded(t, stack))
                    }),
                    Some(ItemSig::Union { variants, .. }) => variants.iter().any(|v| {
                        self.payload_types(*sym, args, v)
                            .iter()
                            .any(|t| self.dies_on_main_thread_guarded(t, stack))
                    }),
                    _ => false,
                };
                stack.pop();
                answer
            }
            _ => false,
        }
    }

    /// Whether `ty` is, or holds through its fields and references, a type `is` picks out.
    fn holds_named(
        &self,
        ty: &Ty,
        is: &dyn Fn(SymbolId) -> bool,
        stack: &mut Vec<SymbolId>,
    ) -> bool {
        match ty {
            Ty::OwnPtr { inner, .. }
            | Ty::Ref { inner, .. }
            | Ty::Optional(inner)
            | Ty::Fallible(inner)
            | Ty::Slice { elem: inner, .. }
            | Ty::ConstSlice(inner)
            | Ty::Array { elem: inner, .. } => self.holds_named(inner, is, stack),
            Ty::Tuple(parts) => parts.iter().any(|t| self.holds_named(t, is, stack)),
            Ty::Named { sym, args } => {
                if is(*sym) {
                    return true;
                }
                if stack.contains(sym) {
                    return false;
                }
                stack.push(*sym);
                let answer = match self.get(*sym) {
                    Some(ItemSig::Struct(_)) => self.with_bases(*sym).into_iter().any(|owner| {
                        let Some(s) = self.struct_sig(owner) else { return false };
                        let own_args = self.args_as(*sym, args, owner);
                        self.field_types(owner, &own_args, &s.fields)
                            .iter()
                            .any(|t| self.holds_named(t, is, stack))
                    }),
                    Some(ItemSig::Union { variants, .. }) => variants.iter().any(|v| {
                        self.payload_types(*sym, args, v)
                            .iter()
                            .any(|t| self.holds_named(t, is, stack))
                    }),
                    _ => false,
                };
                stack.pop();
                answer
            }
            _ => false,
        }
    }

    /// Whether `sym`'s own `drop`, or a base's, is written `#[main_thread]`.
    fn own_drop_on_main_thread(&self, sym: SymbolId) -> bool {
        self.with_bases(sym).into_iter().any(|s| {
            self.drop_impl(s)
                .and_then(|i| self.impls[i].methods.get("drop"))
                .is_some_and(|m| m.main_thread)
        })
    }

    /// Which of §22.2's questions `trait_sym` asks, if it is `Send` or `Share`.
    pub fn crossing_of(&self, trait_sym: SymbolId) -> Option<Crossing> {
        if Some(trait_sym) == self.send_trait {
            Some(Crossing::Send)
        } else if Some(trait_sym) == self.share_trait {
            Some(Crossing::Share)
        } else {
            None
        }
    }

    /// Whether a value of `ty` may move to another thread (spec §22.2).
    pub fn sendable(&self, ty: &Ty) -> bool {
        self.crosses_from_here(ty, Crossing::Send)
    }

    /// Whether a value of `ty` may be read through `&` on several threads at once (§22.2).
    pub fn shareable(&self, ty: &Ty) -> bool {
        self.crosses_from_here(ty, Crossing::Share)
    }

    /// Asked again from inside `named_crosses`, through the bound of `std`'s own
    /// `impl<T: Send> Send for List<T>`, the question carries on with that walk's stack, so a
    /// type that holds a list of itself, `union Json { Array(List<Json>) }`, is answered once
    /// instead of forever: see `CROSS_STACK`, and `COPY_STACK`, which does the same for `Clone`.
    fn crosses_from_here(&self, ty: &Ty, which: Crossing) -> bool {
        let mut stack = CROSS_STACK.with(|s| s.borrow()[which as usize].clone());
        self.crosses(ty, which, &mut stack)
    }

    /// Whether a type synchronises by itself, as `Mutex` and the atomics do, which `std`
    /// says with its private `Synchronises`. A static of such a type may be touched on any
    /// thread (spec §22.5); a static of a plain `u64` is `Share` too, but it is written by
    /// assignment, never through `&`, and a `List` is `Share` but its `push` is no lock.
    pub fn synchronises(&self, ty: &Ty) -> bool {
        let (Some(marker), Ty::Named { sym, .. }) = (self.synchronises_trait, ty) else {
            return false;
        };
        self.hand_impl(*sym, marker)
            .is_some_and(|i| self.impl_applies(&self.impls[i], ty).is_some())
    }

    /// `std`'s own `impl Send` or `impl Share` for `sym`, which replaces the rule from fields.
    /// One written anywhere else is refused (DZ0596) and changes nothing.
    fn hand_impl(&self, sym: SymbolId, trait_sym: SymbolId) -> Option<usize> {
        let head = Some(crate::impls::ImplHead::Named(sym));
        self.impls_by_head.get(&head)?.iter().copied().find(|&i| {
            let imp = &self.impls[i];
            imp.trait_ref.trait_sym == trait_sym && Some(imp.module) == self.std_module
        })
    }

    fn crosses(&self, ty: &Ty, which: Crossing, stack: &mut Vec<SymbolId>) -> bool {
        match ty {
            // What a raw pointer points at may be anyone's, so the compiler cannot say who
            // else reaches it. `own` says the struct owns it, so it is what it points at.
            Ty::Ptr { .. } | Ty::RawCStr => false,
            Ty::OwnPtr { inner, .. } => self.crosses(inner, which, stack),
            // Rust's mapping: a `&T` reaches `T` read-only on the thread it is lent to, so it
            // is what `T` is shared; a `&mut T` is the only way to `T` while it lives, so it
            // moves as `T` does and is never shared, since two threads would both write.
            Ty::Ref { mutable, inner } => self.reference_crosses(*mutable, inner, which, stack),
            Ty::Slice { mutable, elem } => self.reference_crosses(*mutable, elem, which, stack),
            Ty::ConstSlice(elem) | Ty::Array { elem, .. } => self.crosses(elem, which, stack),
            Ty::Optional(inner) | Ty::Fallible(inner) | Ty::Range(inner) => {
                self.crosses(inner, which, stack)
            }
            Ty::Tuple(parts) => parts.iter().all(|t| self.crosses(t, which, stack)),
            // A function on the main thread only is never run anywhere else (§22.5).
            Ty::Fn { main_thread, .. } => !main_thread.0,
            Ty::Param(_) | Ty::Assoc(_) => {
                let Some(want) = (match which {
                    Crossing::Send => self.send_trait,
                    Crossing::Share => self.share_trait,
                }) else {
                    return false;
                };
                self.trait_closure(&self.abstract_bounds(ty), ty)
                    .iter()
                    .any(|r| r.trait_sym == want)
            }
            Ty::Named { sym, args } => self.named_crosses(*sym, args, ty, which, stack),
            // `Self` in a trait's default body stands for every implementer (§7.6).
            Ty::SelfTy => false,
            _ => true,
        }
    }

    /// Why `ty` is not `Send` or not `Share`: the innermost field that is not, with the
    /// path to it, or the type itself where nothing inside is to blame.
    pub fn crossing_blocker(&self, ty: &Ty, which: Crossing) -> Option<Blocker> {
        if self.crosses(ty, which, &mut Vec::new()) {
            return None;
        }
        let mut path = Vec::new();
        let mut at = ty.clone();
        // Down through the first field that does not cross, while one is to blame.
        'down: for _ in 0..16 {
            let Ty::Named { sym, args } = &at else { break };
            if self.own_drop_on_main_thread(*sym) || self.hand_impl_for(*sym, which).is_some() {
                break;
            }
            let Some(ItemSig::Struct(_)) = self.get(*sym) else { break };
            for owner in self.with_bases(*sym) {
                let Some(s) = self.struct_sig(owner) else { continue };
                let own_args = self.args_as(*sym, args, owner);
                let types = self.field_types(owner, &own_args, &s.fields);
                for (f, t) in s.fields.iter().zip(types) {
                    if !self.crosses(&t, which, &mut Vec::new()) {
                        path.push(f.name.clone());
                        at = match t {
                            Ty::OwnPtr { inner, .. } => (*inner).clone(),
                            other => other,
                        };
                        continue 'down;
                    }
                }
            }
            break;
        }
        Some(Blocker { path, ty: at })
    }

    fn hand_impl_for(&self, sym: SymbolId, which: Crossing) -> Option<usize> {
        let t = match which {
            Crossing::Send => self.send_trait,
            Crossing::Share => self.share_trait,
        }?;
        self.hand_impl(sym, t)
    }

    fn reference_crosses(
        &self,
        mutable: bool,
        inner: &Ty,
        which: Crossing,
        stack: &mut Vec<SymbolId>,
    ) -> bool {
        match (which, mutable) {
            (Crossing::Send, true) => self.crosses(inner, Crossing::Send, stack),
            (_, false) => self.crosses(inner, Crossing::Share, stack),
            (Crossing::Share, true) => false,
        }
    }

    fn named_crosses(
        &self,
        sym: SymbolId,
        args: &[Ty],
        ty: &Ty,
        which: Crossing,
        stack: &mut Vec<SymbolId>,
    ) -> bool {
        // A struct cannot contain itself by value; through `own` it can (a list's node),
        // and the answer for the cycle is the one being worked out.
        if stack.contains(&sym) {
            return true;
        }
        let trait_sym = match which {
            Crossing::Send => self.send_trait,
            Crossing::Share => self.share_trait,
        };
        // A value whose drop is the main thread's must die there (spec §22.5).
        if self.own_drop_on_main_thread(sym) {
            return false;
        }
        if let Some(i) = trait_sym.and_then(|t| self.hand_impl(sym, t)) {
            let mut inner = stack.clone();
            inner.push(sym);
            let saved =
                CROSS_STACK.with(|s| std::mem::replace(&mut s.borrow_mut()[which as usize], inner));
            let applies = self.impl_applies(&self.impls[i], ty).is_some();
            CROSS_STACK.with(|s| s.borrow_mut()[which as usize] = saved);
            return applies;
        }
        stack.push(sym);
        let answer = match self.get(sym) {
            Some(ItemSig::Struct(s)) if s.opaque || s.cpp_owned => false,
            Some(ItemSig::Struct(_)) => self.with_bases(sym).into_iter().all(|owner| {
                let Some(s) = self.struct_sig(owner) else { return true };
                let own_args = self.args_as(sym, args, owner);
                self.field_types(owner, &own_args, &s.fields)
                    .iter()
                    .all(|t| self.crosses(t, which, stack))
            }),
            Some(ItemSig::Union { variants, .. }) => variants.iter().all(|v| {
                self.payload_types(sym, args, v).iter().all(|t| self.crosses(t, which, stack))
            }),
            // A trait as a value stands for any implementer, like a type parameter.
            Some(ItemSig::Trait(_)) => false,
            _ => true,
        };
        stack.pop();
        answer
    }
}
