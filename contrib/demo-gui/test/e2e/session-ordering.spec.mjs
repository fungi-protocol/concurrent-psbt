import { assert, withDemoGui } from "./harness.mjs";

await withDemoGui(async (page, { consoleMessages, pageErrors }) => {
  await page.waitForSelector("#sessionForm");
  try {
    await page.waitForFunction(() => document.querySelectorAll(".node.session").length >= 2, null, { timeout: 10_000 });
  } catch (error) {
    const debug = await page.evaluate(() => ({
      readyState: document.readyState,
      sessionCount: document.querySelectorAll(".node.session").length,
      graphChildCount: document.querySelector("#graph")?.childElementCount ?? null,
      seedValue: document.querySelector("#sessionSeed")?.value ?? null,
      moduleScript: document.querySelector('script[type="module"]')?.getAttribute("src") ?? null,
    }));
    throw new Error(
      `demo GUI did not finish rendering: ${error.message}; ` +
      `debug=${JSON.stringify(debug)}; ` +
      `pageErrors=${JSON.stringify(pageErrors)}; ` +
      `console=${JSON.stringify(consoleMessages)}`,
    );
  }

  const deterministic = await page.evaluate(() => {
    const seed = document.querySelector("#sessionSeed");
    return {
      required: seed.required,
      disabled: seed.disabled,
      value: seed.value,
    };
  });
  assert(deterministic.required, "deterministic ordering should require a seed");
  assert(!deterministic.disabled, "deterministic ordering seed input should be enabled");
  assert(/^(?:[0-9a-f]{2})+$/.test(deterministic.value), "deterministic ordering should prefill a hex seed");

  const unset = await page.evaluate(() => {
    const mode = document.querySelector("#sessionOrderingMode");
    const field = document.querySelector("#sessionSeedField");
    const seed = document.querySelector("#sessionSeed");
    const generate = document.querySelector("#generateSessionSeed");
    mode.value = "unset";
    mode.dispatchEvent(new Event("change", { bubbles: true }));
    return {
      visible: !field.hidden,
      required: seed.required,
      disabled: seed.disabled,
      placeholder: seed.placeholder,
      generateDisabled: generate.disabled,
      generateTitle: generate.title,
    };
  });
  assert(unset.visible, "unset ordering should show the optional seed field");
  assert(!unset.required, "unset ordering seed should be optional");
  assert(!unset.disabled, "unset ordering seed input should be enabled");
  assert(/optional/i.test(unset.placeholder), "unset ordering seed placeholder should say optional");
  assert(!unset.generateDisabled, "unset ordering should allow generating an optional seed");
  assert(/optional/i.test(unset.generateTitle), "unset ordering generate button should describe optional seed generation");

  const generatedSeed = await page.evaluate(() => {
    const seed = document.querySelector("#sessionSeed");
    document.querySelector("#generateSessionSeed").click();
    return seed.value;
  });
  assert(/^(?:[0-9a-f]{2})+$/.test(generatedSeed), "unset ordering generate button should produce hex bytes");

  await page.evaluate(async () => {
    const label = document.querySelector("#sessionLabel");
    const seed = document.querySelector("#sessionSeed");
    const form = document.querySelector("#sessionForm");
    label.value = "unset seeded";
    seed.value = "0a0b";
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });

  await page.waitForFunction(() => Boolean(document.querySelector('.node.session[aria-label="session unset seeded"]')));
  const created = await page.evaluate(() => {
    const node = document.querySelector('.node.session[aria-label="session unset seeded"]');
    return {
      hasSeedBadge: Boolean(node?.querySelector(".sort-seed")),
      seedAfterSubmit: document.querySelector("#sessionSeed").value,
    };
  });
  assert(created.hasSeedBadge, "unset ordering session with a seed should show the seed badge");
  assert(created.seedAfterSubmit === "", "unset ordering submit should clear the optional seed field");

  const explicit = await page.evaluate(() => {
    const mode = document.querySelector("#sessionOrderingMode");
    const field = document.querySelector("#sessionSeedField");
    const seed = document.querySelector("#sessionSeed");
    const generate = document.querySelector("#generateSessionSeed");
    seed.value = "beef";
    mode.value = "explicit";
    mode.dispatchEvent(new Event("change", { bubbles: true }));
    return {
      hidden: field.hidden,
      required: seed.required,
      disabled: seed.disabled,
      value: seed.value,
      generateDisabled: generate.disabled,
    };
  });
  assert(explicit.hidden, "explicit ordering should hide the seed field");
  assert(!explicit.required, "explicit ordering should not require a seed");
  assert(explicit.disabled, "explicit ordering should disable the seed input");
  assert(explicit.value === "", "explicit ordering should clear any seed value");
  assert(explicit.generateDisabled, "explicit ordering should disable seed generation");

  const descriptorOverlay = await page.evaluate(async () => {
    const settings = document.querySelector(".descriptor-settings");
    const summary = settings?.querySelector("summary");
    summary?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const panel = document.querySelector(".descriptor-settings-panel");
    const toolbar = document.querySelector(".graph-toolbar");
    const dock = document.querySelector(".descriptor-dock");
    const panelRect = panel?.getBoundingClientRect();
    const toolbarRect = toolbar?.getBoundingClientRect();
    return {
      open: settings?.open ?? false,
      panelTop: panelRect?.top ?? 0,
      panelBottom: panelRect?.bottom ?? 0,
      panelHeight: panelRect?.height ?? 0,
      toolbarBottom: toolbarRect?.bottom ?? 0,
      panelZ: Number.parseInt(getComputedStyle(panel).zIndex || "0", 10),
      toolbarZ: Number.parseInt(getComputedStyle(toolbar).zIndex || "0", 10),
      dockZ: Number.parseInt(getComputedStyle(dock).zIndex || "0", 10),
      descriptorListPresent: Boolean(document.querySelector("#descriptorList")),
    };
  });
  assert(descriptorOverlay.open, "descriptor settings should open from the cog");
  assert(descriptorOverlay.panelTop <= descriptorOverlay.toolbarBottom,
    `descriptor settings should take over enough UI to avoid the workspace toolbar, got panelTop=${descriptorOverlay.panelTop} toolbarBottom=${descriptorOverlay.toolbarBottom}`);
  assert(descriptorOverlay.panelHeight > 240, `descriptor settings should be modal-sized, got height ${descriptorOverlay.panelHeight}`);
  assert(descriptorOverlay.panelZ > descriptorOverlay.toolbarZ,
    `descriptor settings panel should layer above toolbar, got panel z ${descriptorOverlay.panelZ}, toolbar z ${descriptorOverlay.toolbarZ}`);
  assert(descriptorOverlay.dockZ > descriptorOverlay.toolbarZ,
    `descriptor dock stacking context should outrank toolbar while settings are open, got dock z ${descriptorOverlay.dockZ}, toolbar z ${descriptorOverlay.toolbarZ}`);
  assert(!descriptorOverlay.descriptorListPresent, "descriptor settings should not duplicate the per-descriptor UTXO/source listing");

  const descriptorDrawerDirection = await page.evaluate(async () => {
    const settings = document.querySelector(".descriptor-settings");
    if (settings) settings.open = false;
    const drawer = document.querySelector(".descriptor-chip:not(.unrecognized) .descriptor-drawer");
    const summary = drawer?.querySelector("summary");
    summary?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const body = drawer?.querySelector(".descriptor-drawer-body");
    const bodyRect = body?.getBoundingClientRect();
    const summaryRect = summary?.getBoundingClientRect();
    return {
      open: drawer?.open ?? false,
      bodyBottom: bodyRect?.bottom ?? 0,
      bodyTop: bodyRect?.top ?? 0,
      summaryTop: summaryRect?.top ?? 0,
      bodyPosition: body ? getComputedStyle(body).position : "",
    };
  });
  assert(descriptorDrawerDirection.open, "descriptor drawer should open from its summary");
  assert(["absolute", "fixed"].includes(descriptorDrawerDirection.bodyPosition),
    `descriptor drawer body should be positioned as an upward popover, got ${descriptorDrawerDirection.bodyPosition}`);
  assert(descriptorDrawerDirection.bodyBottom <= descriptorDrawerDirection.summaryTop,
    `descriptor drawer should open upward from the bottom dock, got bodyBottom=${descriptorDrawerDirection.bodyBottom}, summaryTop=${descriptorDrawerDirection.summaryTop}`);
  assert(descriptorDrawerDirection.bodyTop >= 0,
    `descriptor drawer should remain onscreen when opened upward, got top ${descriptorDrawerDirection.bodyTop}`);

  const amountTone = await page.evaluate(() => {
    const scale = [...document.querySelectorAll(".amount-scale")]
      .find((candidate) => candidate.nextElementSibling?.textContent?.trim());
    const significant = scale?.nextElementSibling;
    const scaleStyle = scale ? getComputedStyle(scale) : null;
    const significantStyle = significant ? getComputedStyle(significant) : null;
    return {
      scaleText: scale?.textContent || "",
      significantText: significant?.textContent || "",
      scaleFill: scaleStyle?.fill || "",
      scaleOpacity: scaleStyle?.opacity || "",
      significantFill: significantStyle?.fill || "",
      significantOpacity: significantStyle?.opacity || "",
    };
  });
  assert(amountTone.scaleText, "expected a muted leading-zero amount span");
  assert(amountTone.significantText, "expected significant amount digits after the muted leading-zero span");
  assert(
    amountTone.scaleFill === amountTone.significantFill,
    `expected muted leading zeros to use significant digit fill ${amountTone.significantFill}, got ${amountTone.scaleFill}`,
  );
  assert(Number(amountTone.scaleOpacity) < Number(amountTone.significantOpacity || 1), "expected muted leading zeros to use lower opacity");

  const coloredAmountTone = await page.evaluate(() => {
    const scale = document.querySelector(".psbt-balance-delta.deficit .amount-scale");
    const significant = scale?.nextElementSibling;
    const scaleStyle = scale ? getComputedStyle(scale) : null;
    const significantStyle = significant ? getComputedStyle(significant) : null;
    return {
      scaleText: scale?.textContent || "",
      significantText: significant?.textContent || "",
      scaleFill: scaleStyle?.fill || "",
      scaleOpacity: scaleStyle?.opacity || "",
      significantFill: significantStyle?.fill || "",
      significantOpacity: significantStyle?.opacity || "",
    };
  });
  assert(coloredAmountTone.scaleText, "expected a colored deficit amount with muted leading zeros");
  assert(coloredAmountTone.significantText, "expected significant deficit digits after muted leading zeros");
  assert(
    coloredAmountTone.scaleFill === coloredAmountTone.significantFill,
    `expected colored muted leading zeros to inherit ${coloredAmountTone.significantFill}, got ${coloredAmountTone.scaleFill}`,
  );
  assert(
    Number(coloredAmountTone.scaleOpacity) < Number(coloredAmountTone.significantOpacity || 1),
    "expected colored muted leading zeros to use lower opacity only",
  );

  const sizeUnitCycle = await page.evaluate(async () => {
    const before = [...document.querySelectorAll(".psbt-size-total")].map((node) => node.textContent.trim());
    const target = document.querySelector(".psbt-size-total");
    const subtotal = document.querySelector(".psbt-section-total");
    const targetStyle = target ? getComputedStyle(target) : null;
    const subtotalStyle = subtotal ? getComputedStyle(subtotal) : null;
    target?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return {
      toolbarSelectPresent: Boolean(document.querySelector("#sizeUnit")),
      targetRole: target?.getAttribute("role") || "",
      targetTabindex: target?.getAttribute("tabindex") || "",
      targetFontSize: targetStyle?.fontSize || "",
      subtotalFontSize: subtotalStyle?.fontSize || "",
      targetFontWeight: targetStyle?.fontWeight || "",
      subtotalFontWeight: subtotalStyle?.fontWeight || "",
      before,
      after: [...document.querySelectorAll(".psbt-size-total")].map((node) => node.textContent.trim()),
    };
  });
  assert(!sizeUnitCycle.toolbarSelectPresent, "size unit selector should be eliminated from the toolbar");
  assert(sizeUnitCycle.targetRole === "button", `expected size estimate to be the unit toggle button, got role ${sizeUnitCycle.targetRole}`);
  assert(sizeUnitCycle.targetTabindex === "0", `expected size estimate toggle to be keyboard focusable, got tabindex ${sizeUnitCycle.targetTabindex}`);
  assert(sizeUnitCycle.targetFontSize === sizeUnitCycle.subtotalFontSize,
    `expected size estimate font size ${sizeUnitCycle.targetFontSize} to match subtotal font size ${sizeUnitCycle.subtotalFontSize}`);
  assert(sizeUnitCycle.targetFontWeight === sizeUnitCycle.subtotalFontWeight,
    `expected size estimate font weight ${sizeUnitCycle.targetFontWeight} to match subtotal font weight ${sizeUnitCycle.subtotalFontWeight}`);
  assert(sizeUnitCycle.before.length > 0 && sizeUnitCycle.before.every((text) => text.includes("vB")),
    `expected initial size totals in vB, got ${JSON.stringify(sizeUnitCycle.before)}`);
  assert(sizeUnitCycle.after.length === sizeUnitCycle.before.length && sizeUnitCycle.after.every((text) => text.includes("WU")),
    `expected clicking a size estimate to globally cycle totals to WU, got ${JSON.stringify(sizeUnitCycle.after)}`);

  const atomicFragmentActions = await page.evaluate(async () => {
    const fragment = [...document.querySelectorAll(".node.fragment")]
      .find((node) => node.querySelectorAll(".coin-row").length === 1);
    fragment?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const atomize = document.querySelector("#atomizeSelected");
    return {
      fragmentFound: Boolean(fragment),
      atomizeText: atomize?.textContent?.trim() || "",
      atomizeHidden: atomize?.hidden ?? false,
      splitButtonPresent: Boolean(document.querySelector("#splitSelected")),
    };
  });
  assert(atomicFragmentActions.fragmentFound, "expected demo to include an atomic fragment");
  assert(atomicFragmentActions.atomizeText === "Atomize PSBT", `expected split action to be renamed, got ${atomicFragmentActions.atomizeText}`);
  assert(atomicFragmentActions.atomizeHidden, "atomic PSBT fragments should not be atomizable");
  assert(!atomicFragmentActions.splitButtonPresent, "old Split PSBT button id should not remain in the UI");

  const expandedCoinSizing = await page.evaluate(async () => {
    const session = document.querySelector(".node.session");
    const body = session?.querySelector(":scope > rect.node-body");
    const before = {
      width: Number(body?.getAttribute("width") || 0),
      height: Number(body?.getAttribute("height") || 0),
    };
    const coin = session?.querySelector(".coin-row");
    coin?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const expandedSession = document.querySelector(".node.session");
    const expandedBody = expandedSession?.querySelector(":scope > rect.node-body");
    return {
      before,
      after: {
        width: Number(expandedBody?.getAttribute("width") || 0),
        height: Number(expandedBody?.getAttribute("height") || 0),
      },
      details: [...expandedSession.querySelectorAll(".coin-detail")].map((line) => line.textContent.trim()),
      overflowMarkers: [...expandedSession.querySelectorAll("text")]
        .map((line) => line.textContent.trim())
        .filter((text) => /^\+\d+ row/.test(text)),
    };
  });
  assert(expandedCoinSizing.after.width > expandedCoinSizing.before.width,
    `expected expanded coin details to widen the session, got ${expandedCoinSizing.before.width}->${expandedCoinSizing.after.width}`);
  assert(expandedCoinSizing.after.height > expandedCoinSizing.before.height,
    `expected expanded coin details to grow the session, got ${expandedCoinSizing.before.height}->${expandedCoinSizing.after.height}`);
  assert(expandedCoinSizing.details.some((line) => line.startsWith("outpoint ")), "expected expanded input details to include outpoint");
  assert(expandedCoinSizing.details.some((line) => line.startsWith("nSequence ")), "expected expanded input details to include nSequence");
  assert(!expandedCoinSizing.details.some((line) => line.startsWith("label ")), "expected unlabeled inputs not to invent a label");
  assert(expandedCoinSizing.overflowMarkers.length === 0, "expanded details should fit without row overflow markers");

  const explicitFeeSubtotalNotes = await page.evaluate(() => [...document.querySelectorAll(".psbt-explicit-fee-subtotal-note")].map((note) => {
    const parent = note.parentElement;
    const noteStyle = getComputedStyle(note);
    const parentStyle = parent ? getComputedStyle(parent) : null;
    return {
      text: note.textContent?.trim() || "",
      parentClass: parent?.getAttribute("class") || "",
      noteFontSize: Number.parseFloat(noteStyle.fontSize || "0"),
      parentFontSize: Number.parseFloat(parentStyle?.fontSize || "0"),
    };
  }));
  assert(explicitFeeSubtotalNotes.length > 0, "expected output subtotals with explicit fee contributions to say '(incl. fees)'");
  for (const note of explicitFeeSubtotalNotes) {
    assert(note.text === "(incl. fees)", `expected explicit fee subtotal note text, got ${note.text}`);
    assert(note.parentClass.includes("psbt-section-total"), `expected subtotal note to live in a subtotal amount, got ${note.parentClass}`);
    assert(note.noteFontSize < note.parentFontSize, `expected note font ${note.noteFontSize} to be smaller than subtotal font ${note.parentFontSize}`);
  }

  const feeRateSignals = await page.evaluate(() => ({
    labels: [...document.querySelectorAll(".node.session .psbt-balance-delta-label")].map((label) => label.textContent?.trim() || ""),
    signals: [...document.querySelectorAll(".node.session .fee-rate-signal")].map((signal) => ({
      text: signal.textContent?.trim() || "",
      sectionKind: signal.getAttribute("data-section-kind") || "",
      descriptorId: signal.getAttribute("data-descriptor-id") || "",
    })),
  }));
  assert(!feeRateSignals.labels.includes("explicit / surplus"), "surplus label should describe accounted fees, not expose the raw field name");
  assert(feeRateSignals.labels.includes("accounted / surplus"), "expected clearer accounted/surplus label");
  assert(feeRateSignals.signals.some((signal) => signal.sectionKind === "recognized" && signal.descriptorId !== "alice"),
    `expected non-mine descriptor fee-rate signal, got ${JSON.stringify(feeRateSignals.signals)}`);
  assert(feeRateSignals.signals.some((signal) => signal.sectionKind === "whole"),
    `expected whole-transaction fee-rate signal, got ${JSON.stringify(feeRateSignals.signals)}`);

  const subtotalAndFeeOverlaps = await page.evaluate(() => {
    const selectors = [
      ".psbt-section-total",
      ".psbt-balance-delta-label",
      ".psbt-balance-delta",
      ".fee-rate-signal",
      ".fee-finalize-button text",
    ].join(",");
    const entries = [...document.querySelectorAll(`.node.session ${selectors}, .node.fragment ${selectors}`)]
      .map((element, index) => {
        const rect = element.getBoundingClientRect();
        const node = element.closest(".node");
        return {
          index,
          node: node?.getAttribute("aria-label") || node?.getAttribute("class") || "",
          text: element.textContent?.trim() || "",
          className: element.getAttribute("class") || element.parentElement?.getAttribute("class") || "",
          left: rect.left,
          right: rect.right,
          top: rect.top,
          bottom: rect.bottom,
          width: rect.width,
          height: rect.height,
        };
      })
      .filter((entry) => entry.text && entry.width > 0 && entry.height > 0);
    const overlapArea = (left, right) => {
      const width = Math.min(left.right, right.right) - Math.max(left.left, right.left);
      const height = Math.min(left.bottom, right.bottom) - Math.max(left.top, right.top);
      return width > 1 && height > 1 ? width * height : 0;
    };
    const overlaps = [];
    for (let leftIndex = 0; leftIndex < entries.length; leftIndex += 1) {
      for (let rightIndex = leftIndex + 1; rightIndex < entries.length; rightIndex += 1) {
        const left = entries[leftIndex];
        const right = entries[rightIndex];
        if (left.node !== right.node) continue;
        const area = overlapArea(left, right);
        if (area > 0) overlaps.push({ area, left, right });
      }
    }
    return overlaps;
  });
  assert(subtotalAndFeeOverlaps.length === 0,
    `expected fee-rate and subtotal labels not to overlap, got ${JSON.stringify(subtotalAndFeeOverlaps.slice(0, 3))}`);

  const mineSubtransactionBackgrounds = await page.evaluate(() => [...document.querySelectorAll(".psbt-subtxn-background.mine")].map((background) => {
    const style = getComputedStyle(background);
    const section = background.closest(".psbt-subtxn");
    return {
      sectionClass: section?.getAttribute("class") || "",
      fill: style.fill,
      fillOpacity: Number.parseFloat(style.fillOpacity || "0"),
      stroke: style.stroke,
      strokeWidth: Number.parseFloat(style.strokeWidth || "0"),
      width: Number(background.getAttribute("width") || 0),
      height: Number(background.getAttribute("height") || 0),
      firstInSection: section?.firstElementChild === background,
    };
  }));
  assert(mineSubtransactionBackgrounds.length > 0, "expected mine descriptor sub-transactions to have subtle background emphasis");
  for (const background of mineSubtransactionBackgrounds) {
    assert(background.sectionClass.includes("mine"), `expected background to live in a mine section, got ${background.sectionClass}`);
    assert(background.fill !== "none", "expected mine sub-transaction background to use a fill");
    assert(background.fillOpacity > 0 && background.fillOpacity <= 0.2,
      `expected subtle background opacity, got ${background.fillOpacity}`);
    assert(background.stroke === "none" || background.strokeWidth === 0,
      `expected no additional background border, got stroke ${background.stroke} width ${background.strokeWidth}`);
    assert(background.width > 0 && background.height > 0,
      `expected mine sub-transaction background to have area, got ${background.width}x${background.height}`);
    assert(background.firstInSection, "expected mine sub-transaction background behind section content");
  }

  const initialFeePanel = await page.evaluate(async () => {
    const button = document.querySelector(".fee-finalize-button");
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const panel = document.querySelector("#feeContributionPanel");
    const slider = document.querySelector("#feeContributionSlider");
    const amount = document.querySelector("#feeContributionAmount");
    const rate = document.querySelector("#feeContributionRate");
    const comparison = document.querySelector("#feeContributionComparison");
    const confirmRow = document.querySelector("#feeContributionConfirmRow");
    const confirm = document.querySelector("#feeContributionConfirm");
    const apply = document.querySelector("#feeContributionApply");
    return {
      buttonFound: Boolean(button),
      hidden: panel?.hidden ?? true,
      panelClass: panel?.className || "",
      sliderMin: slider?.min || "",
      sliderMax: Number(slider?.max || 0),
      sliderValue: Number(slider?.value || 0),
      amountValue: Number(amount?.value || 0),
      rateText: rate?.textContent?.trim() || "",
      comparisonText: comparison?.textContent?.trim() || "",
      confirmHidden: confirmRow?.hidden ?? true,
      confirmChecked: confirm?.checked ?? true,
      applyDisabled: apply?.disabled ?? false,
    };
  });
  assert(initialFeePanel.buttonFound, "expected a finalize fee control in a mine descriptor section");
  assert(!initialFeePanel.hidden, "finalize fee should open a fee contribution panel");
  assert(initialFeePanel.sliderMin === "0", `expected slider min 0, got ${initialFeePanel.sliderMin}`);
  assert(initialFeePanel.sliderMax > 0, "expected slider max to be the available surplus");
  assert(initialFeePanel.sliderValue === initialFeePanel.sliderMax, "expected slider to default to the available surplus");
  assert(initialFeePanel.amountValue === initialFeePanel.sliderValue, "expected numeric amount to mirror the slider");
  assert(initialFeePanel.rateText.includes("sat/vB"), `expected feerate readout, got ${initialFeePanel.rateText}`);
  assert(initialFeePanel.comparisonText.includes("overall"), `expected overall feerate comparison, got ${initialFeePanel.comparisonText}`);
  assert(initialFeePanel.panelClass.includes("warning-confirm"), `expected mandatory confirmation warning class, got ${initialFeePanel.panelClass}`);
  assert(!initialFeePanel.confirmHidden, "feerates above 1000 sat/vB should show mandatory confirmation");
  assert(!initialFeePanel.confirmChecked, "mandatory confirmation should start unchecked");
  assert(initialFeePanel.applyDisabled, "mandatory confirmation should disable apply until checked");

  const zeroFeePanel = await page.evaluate(async () => {
    const slider = document.querySelector("#feeContributionSlider");
    slider.value = "0";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const panel = document.querySelector("#feeContributionPanel");
    const confirmRow = document.querySelector("#feeContributionConfirmRow");
    const apply = document.querySelector("#feeContributionApply");
    return {
      panelClass: panel?.className || "",
      confirmHidden: confirmRow?.hidden ?? false,
      applyDisabled: apply?.disabled ?? true,
    };
  });
  assert(zeroFeePanel.panelClass.includes("warning-none"), `expected no warning at zero fee, got ${zeroFeePanel.panelClass}`);
  assert(zeroFeePanel.confirmHidden, "zero fee should not require mandatory confirmation");
  assert(!zeroFeePanel.applyDisabled, "zero fee should be applyable without confirmation");

  const badgeTooltips = await page.evaluate(() => [...document.querySelectorAll(".psbt-status-badge")].map((badge) => {
    const hitTarget = badge.querySelector(".psbt-status-hit");
    const title = badge.querySelector("title")?.textContent?.trim() || "";
    const hitTitle = hitTarget?.querySelector("title")?.textContent?.trim() || "";
    const hitStyle = hitTarget ? getComputedStyle(hitTarget) : null;
    return {
      title,
      hitTitle,
      ariaLabel: badge.getAttribute("aria-label") || "",
      role: badge.getAttribute("role") || "",
      hitWidth: Number(hitTarget?.getAttribute("width") || 0),
      hitHeight: Number(hitTarget?.getAttribute("height") || 0),
      hitPointerEvents: hitStyle?.pointerEvents || "",
    };
  }));
  assert(badgeTooltips.length > 0, "expected rendered PSBT status badges");
  for (const badge of badgeTooltips) {
    assert(badge.title, "expected each PSBT status badge to have tooltip text");
    assert(badge.hitTitle === badge.title, `expected badge hit target title to match ${badge.title}, got ${badge.hitTitle}`);
    assert(badge.ariaLabel === badge.title, `expected badge aria-label to match ${badge.title}, got ${badge.ariaLabel}`);
    assert(badge.role === "img", `expected badge role img, got ${badge.role}`);
    assert(badge.hitWidth >= 18 && badge.hitHeight >= 18, `expected badge hit target at least 18x18, got ${badge.hitWidth}x${badge.hitHeight}`);
    assert(badge.hitPointerEvents !== "none", "expected badge hit target to receive pointer events");
  }

  const globalFields = await page.evaluate(() => {
    const collect = (selector) => [...(document.querySelector(selector)?.querySelectorAll(".psbt-global-field") || [])]
      .map((row) => ({
        label: row.querySelector(".psbt-global-label")?.textContent?.trim() || "",
        value: row.querySelector(".psbt-global-value")?.textContent?.trim() || "",
      }));
    return {
      session: collect(".node.session"),
      fragment: collect(".node.fragment"),
    };
  });
  assert(globalFields.session.some((row) => row.label === "format" && row.value === "unordered"),
    `expected session PSBT global fields to include unordered format, got ${JSON.stringify(globalFields.session)}`);
  assert(globalFields.session.some((row) => row.label === "tx" && /v2.*lock 0/.test(row.value)),
    `expected session PSBT global fields to include tx version and locktime, got ${JSON.stringify(globalFields.session)}`);
  assert(globalFields.session.some((row) => row.label === "sort" && /det.*seed/.test(row.value)),
    `expected session PSBT global fields to include deterministic sort seed, got ${JSON.stringify(globalFields.session)}`);
  assert(globalFields.fragment.some((row) => row.label === "format" && row.value === "unordered"),
    `expected fragment PSBT global fields to include unordered format, got ${JSON.stringify(globalFields.fragment)}`);
  assert(globalFields.fragment.some((row) => row.label === "tx" && /v2.*lock 0/.test(row.value)),
    `expected fragment PSBT global fields to include tx version and locktime, got ${JSON.stringify(globalFields.fragment)}`);

  const inspectorRows = async () => page.evaluate(() => {
    const rows = {};
    const terms = [...document.querySelectorAll("#inspector dt")];
    for (const term of terms) {
      rows[term.textContent.trim()] = term.nextElementSibling?.textContent?.trim() || "";
    }
    return rows;
  });

  await page.evaluate(() => {
    document.querySelector(".node.session")?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
  });
  await page.waitForFunction(() => [...document.querySelectorAll("#inspector dt")]
    .some((term) => term.textContent.trim() === "unique id"));
  const sessionRows = await inspectorRows();
  assert(!("txid" in sessionRows), "unordered session should not display a txid");
  assert(/^[0-9a-f]{64}$/.test(sessionRows["unique id"]), `expected 64-hex session unique id, got ${sessionRows["unique id"]}`);
  assert(sessionRows["identity source"] === "psbt.md unordered PSBT unique id",
    `expected psbt.md unique-id source, got ${sessionRows["identity source"]}`);

  await page.evaluate(() => {
    document.querySelector(".node.fragment")?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, view: window }));
  });
  await page.waitForFunction(() => document.querySelector("#orderSelected") && !document.querySelector("#orderSelected").hidden);
  await page.evaluate(() => document.querySelector("#orderSelected").click());
  await page.waitForFunction(() => [...document.querySelectorAll("#inspector dt")]
    .some((term) => term.textContent.trim() === "txid"));
  const fixedRows = await inspectorRows();
  assert(!("unique id" in fixedRows), "ordered non-modifiable SegWit candidate should display a txid instead of a PSBT unique id");
  assert(/^[0-9a-f]{64}$/.test(fixedRows.txid), `expected 64-hex txid, got ${fixedRows.txid}`);
  assert(fixedRows["identity source"] === "ordered non-modifiable SegWit transaction",
    `expected fixed SegWit txid source, got ${fixedRows["identity source"]}`);

  assert(pageErrors.length === 0, `demo GUI raised uncaught page errors: ${pageErrors.join(" | ")}`);
  console.log("OK: demo GUI rendered in store Chromium and session ordering seed controls behaved correctly");
});
