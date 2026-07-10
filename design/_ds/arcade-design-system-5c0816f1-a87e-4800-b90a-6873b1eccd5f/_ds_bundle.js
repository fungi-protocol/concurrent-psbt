/* @ds-bundle: {"format":3,"namespace":"ArcadeDesignSystem_5c0816","components":[],"sourceHashes":{"ui_kits/mobile/components.jsx":"f89ad0ec29f7","ui_kits/mobile/dispute-screens.jsx":"4053c0988147","ui_kits/mobile/ios-frame.jsx":"d67eb3ffe562"},"inlinedExternals":[],"unexposedExports":[]} */

(() => {

const __ds_ns = (window.ArcadeDesignSystem_5c0816 = window.ArcadeDesignSystem_5c0816 || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// ui_kits/mobile/components.jsx
try { (() => {
// Arcade mobile UI kit components

function Icon({
  name,
  size = 24,
  color,
  style = {}
}) {
  // Icons are black by default. For white icons, pass color="white" — uses filter invert.
  const url = `../../assets/icons/${name}${size}.svg`;
  const filter = color === 'white' || color === '#fff' ? 'invert(1)' : color === '#959595' || color === 'muted' ? 'opacity(0.55)' : 'none';
  return /*#__PURE__*/React.createElement("img", {
    src: url,
    width: size,
    height: size,
    style: {
      display: 'inline-block',
      filter,
      ...style
    }
  });
}
function ArcadeStatusBar({
  dark
}) {
  const c = dark ? '#fff' : '#000';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      padding: '14px 22px 6px',
      fontFamily: '"Cash Sans", -apple-system, system-ui',
      fontWeight: 600,
      fontSize: 15,
      color: c
    }
  }, /*#__PURE__*/React.createElement("span", null, "9:41"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6,
      alignItems: 'center'
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "18",
    height: "11",
    viewBox: "0 0 18 11"
  }, /*#__PURE__*/React.createElement("rect", {
    x: "0",
    y: "7",
    width: "3",
    height: "4",
    rx: "0.6",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "4.5",
    y: "5",
    width: "3",
    height: "6",
    rx: "0.6",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "9",
    y: "2.5",
    width: "3",
    height: "8.5",
    rx: "0.6",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "13.5",
    y: "0",
    width: "3",
    height: "11",
    rx: "0.6",
    fill: c
  })), /*#__PURE__*/React.createElement("svg", {
    width: "24",
    height: "12",
    viewBox: "0 0 24 12"
  }, /*#__PURE__*/React.createElement("rect", {
    x: "0.5",
    y: "0.5",
    width: "21",
    height: "11",
    rx: "3",
    stroke: c,
    strokeOpacity: "0.4",
    fill: "none"
  }), /*#__PURE__*/React.createElement("rect", {
    x: "2",
    y: "2",
    width: "18",
    height: "8",
    rx: "1.5",
    fill: c
  }))));
}
function CashAmount({
  amount = '0',
  sub,
  size = 72,
  color = '#000'
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'baseline',
      justifyContent: 'center',
      gap: 2
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: size * 0.5,
      color: '#959595',
      fontFamily: '"Cash Sans"'
    }
  }, "$"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: size,
      fontFamily: '"Cash Sans"',
      fontWeight: 400,
      letterSpacing: '-0.02em',
      lineHeight: 1,
      color
    }
  }, amount), sub && /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: size * 0.4,
      color: '#959595',
      fontFamily: '"Cash Sans"'
    }
  }, ".", sub));
}
function Button({
  variant = 'prominent',
  children,
  onClick,
  icon,
  disabled
}) {
  const styles = {
    prominent: {
      bg: '#00D64F',
      fg: '#fff'
    },
    standard: {
      bg: '#E8E8E8',
      fg: '#000'
    },
    subtle: {
      bg: 'transparent',
      fg: '#000'
    },
    danger: {
      bg: '#D7040E',
      fg: '#fff'
    }
  };
  const s = styles[variant] || styles.prominent;
  return /*#__PURE__*/React.createElement("button", {
    onClick: onClick,
    disabled: disabled,
    style: {
      height: 52,
      padding: '0 24px',
      borderRadius: 9999,
      border: 0,
      background: disabled ? '#E8E8E8' : s.bg,
      color: disabled ? '#959595' : s.fg,
      fontFamily: '"CashMarket", "Cash Sans"',
      fontWeight: 500,
      fontSize: 16,
      letterSpacing: '0.005em',
      cursor: disabled ? 'default' : 'pointer',
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 8,
      width: '100%',
      transition: 'opacity 120ms'
    }
  }, icon, children);
}
function ActivityCell({
  iconName,
  title,
  subtitle,
  amount,
  incoming,
  onClick
}) {
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 12,
      padding: '12px 16px',
      cursor: 'pointer',
      borderRadius: 16,
      background: '#fff'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 44,
      height: 44,
      borderRadius: 9999,
      background: '#F2F2F2',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      flex: '0 0 44px'
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: iconName,
    size: 24
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      fontWeight: 500,
      color: '#000',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis'
    }
  }, title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666'
    }
  }, subtitle)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      fontWeight: 500,
      color: incoming ? '#00893A' : '#000',
      fontVariantNumeric: 'tabular-nums'
    }
  }, amount));
}
function TabBar({
  active,
  onChange
}) {
  const tabs = [{
    id: 'money',
    label: 'Money',
    icon: 'navigationMoney'
  }, {
    id: 'card',
    label: 'Card',
    icon: 'navigationCard'
  }, {
    id: 'activity',
    label: 'Activity',
    icon: 'navigationActivity'
  }, {
    id: 'btc',
    label: 'Bitcoin',
    icon: 'navigationBitcoin'
  }, {
    id: 'discover',
    label: 'Discover',
    icon: 'navigationDiscover'
  }];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'space-around',
      padding: '8px 8px 22px',
      background: 'rgba(255,255,255,0.92)',
      backdropFilter: 'blur(20px)',
      borderTop: '0.5px solid #E8E8E8'
    }
  }, tabs.map(t => /*#__PURE__*/React.createElement("button", {
    key: t.id,
    onClick: () => onChange(t.id),
    style: {
      border: 0,
      background: 'transparent',
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: 2,
      cursor: 'pointer',
      padding: '4px 8px',
      color: active === t.id ? '#000' : '#959595'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: `../../assets/icons/${t.icon}.svg`,
    width: 28,
    height: 28,
    style: {
      opacity: active === t.id ? 1 : 0.5
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      fontWeight: 500,
      fontFamily: '"Cash Sans"'
    }
  }, t.label))));
}
function QuickAction({
  iconName,
  label,
  onClick
}) {
  return /*#__PURE__*/React.createElement("button", {
    onClick: onClick,
    style: {
      flex: 1,
      border: 0,
      background: '#F2F2F2',
      borderRadius: 20,
      padding: '16px 8px',
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: 8,
      cursor: 'pointer',
      fontFamily: '"Cash Sans"'
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: iconName,
    size: 24
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 13,
      fontWeight: 500,
      color: '#000'
    }
  }, label));
}
function HomeScreen({
  balance,
  onNav
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '8px 16px 100px',
      background: '#fff',
      minHeight: '100%'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      padding: '8px 4px 20px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 36,
      height: 36,
      borderRadius: 9999,
      background: '#F2F2F2'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 12
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "qr",
    size: 24
  }), /*#__PURE__*/React.createElement(Icon, {
    name: "notifications",
    size: 24
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '24px 0 28px',
      textAlign: 'center'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666',
      fontWeight: 500,
      marginBottom: 6,
      textTransform: 'uppercase',
      letterSpacing: '0.04em'
    }
  }, "Cash balance"), /*#__PURE__*/React.createElement(CashAmount, {
    amount: balance.main,
    sub: balance.cents
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 10,
      marginBottom: 24
    }
  }, /*#__PURE__*/React.createElement(Button, {
    variant: "prominent",
    onClick: () => onNav('add')
  }, "Add cash"), /*#__PURE__*/React.createElement(Button, {
    variant: "standard",
    onClick: () => onNav('out')
  }, "Cash Out")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 10,
      marginBottom: 24
    }
  }, /*#__PURE__*/React.createElement(QuickAction, {
    iconName: "send",
    label: "Send",
    onClick: () => onNav('send')
  }), /*#__PURE__*/React.createElement(QuickAction, {
    iconName: "deposit",
    label: "Request",
    onClick: () => onNav('request')
  }), /*#__PURE__*/React.createElement(QuickAction, {
    iconName: "gift",
    label: "Gift",
    onClick: () => onNav('gift')
  }), /*#__PURE__*/React.createElement(QuickAction, {
    iconName: "magic",
    label: "More",
    onClick: () => {}
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 10,
      marginBottom: 24,
      overflowX: 'auto'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 180,
      background: '#00D64F',
      borderRadius: 20,
      padding: 16,
      display: 'flex',
      flexDirection: 'column',
      justifyContent: 'space-between',
      height: 140,
      color: '#000'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/illustrations/rainbow.svg",
    style: {
      width: 56,
      height: 56
    }
  }), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 16,
      fontWeight: 500
    }
  }, "Set a savings goal"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      opacity: 0.7
    }
  }, "Earn up to 4.5% APY"))), /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 180,
      background: '#000',
      borderRadius: 20,
      padding: 16,
      display: 'flex',
      flexDirection: 'column',
      justifyContent: 'space-between',
      height: 140,
      color: '#fff'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/illustrations/bitcoin-lock.svg",
    style: {
      width: 56,
      height: 56
    }
  }), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 16,
      fontWeight: 500
    }
  }, "Buy Bitcoin"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      opacity: 0.7
    }
  }, "From $1"))), /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 180,
      background: '#CCFF14',
      borderRadius: 20,
      padding: 16,
      display: 'flex',
      flexDirection: 'column',
      justifyContent: 'space-between',
      height: 140,
      color: '#000'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/illustrations/cash-stack.svg",
    style: {
      width: 56,
      height: 56
    }
  }), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 16,
      fontWeight: 500
    }
  }, "Direct deposit"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      opacity: 0.7
    }
  }, "Up to 2 days early")))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '0 4px 8px',
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'baseline'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 20,
      fontWeight: 500,
      color: '#000'
    }
  }, "Activity"), /*#__PURE__*/React.createElement("a", {
    style: {
      fontSize: 14,
      color: '#00893A',
      fontWeight: 500,
      cursor: 'pointer'
    },
    onClick: () => onNav('activity')
  }, "See all")), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff'
    }
  }, /*#__PURE__*/React.createElement(ActivityCell, {
    iconName: "deposit",
    title: "Direct deposit",
    subtitle: "Today \xB7 Acme Corp",
    amount: "+ $2,847.00",
    incoming: true
  }), /*#__PURE__*/React.createElement(ActivityCell, {
    iconName: "transferP2P",
    title: "Sent to Mia",
    subtitle: "Yesterday \xB7 For pizza \uD83C\uDF55",
    amount: "\u2013 $24.00"
  }), /*#__PURE__*/React.createElement(ActivityCell, {
    iconName: "cardBasic",
    title: "Trader Joe's",
    subtitle: "Apr 14 \xB7 Groceries",
    amount: "\u2013 $62.14"
  })));
}
Object.assign(window, {
  Icon,
  ArcadeStatusBar,
  CashAmount,
  Button,
  ActivityCell,
  TabBar,
  QuickAction,
  HomeScreen
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/mobile/components.jsx", error: String((e && e.message) || e) }); }

// ui_kits/mobile/dispute-screens.jsx
try { (() => {
// Cash App dispute flow screens
// Adaptive flow: instead of asking the user to self-classify their dispute reason,
// we ask "Do you recognize this transaction?" then ask context questions while
// surfacing merchant data to help jog memory.

// ───── shared bits ─────
const TX = {
  merchant: 'SQ *Verve Coffee',
  cleanName: 'Verve Coffee Roasters',
  category: 'Coffee shop',
  amount: 5.42,
  date: 'Apr 15, 2026',
  time: '8:42 AM',
  location: 'Santa Cruz, CA',
  card: '···· 4317',
  priorCount: 3
};
function NavBar({
  title,
  onBack,
  onClose
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      padding: '6px 16px 10px',
      minHeight: 44
    }
  }, onBack ? /*#__PURE__*/React.createElement("button", {
    onClick: onBack,
    style: {
      border: 0,
      background: 'transparent',
      padding: 8,
      cursor: 'pointer',
      display: 'flex'
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "22",
    height: "22",
    viewBox: "0 0 22 22"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M14 4l-8 7 8 7",
    stroke: "#000",
    strokeWidth: "2",
    fill: "none",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }))) : /*#__PURE__*/React.createElement("div", {
    style: {
      width: 38
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      fontWeight: 500,
      color: '#000',
      letterSpacing: '-0.005em'
    }
  }, title), /*#__PURE__*/React.createElement("button", {
    onClick: onClose,
    style: {
      border: 0,
      background: 'transparent',
      padding: 8,
      cursor: 'pointer'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/clear24.svg",
    width: 22,
    height: 22
  })));
}
function ContinueBar({
  label = 'Continue',
  onClick,
  disabled,
  secondary,
  secondaryLabel = 'Back',
  onSecondary
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '12px 16px 24px',
      background: '#F5F5F5',
      borderTop: '0.5px solid #E8E8E8',
      display: 'flex',
      gap: 10
    }
  }, secondary && /*#__PURE__*/React.createElement("button", {
    onClick: onSecondary,
    style: {
      flex: 1,
      height: 56,
      borderRadius: 9999,
      border: 0,
      background: '#E8E8E8',
      color: '#000',
      fontFamily: '"CashMarket","Cash Sans"',
      fontSize: 16,
      fontWeight: 500,
      cursor: 'pointer'
    }
  }, secondaryLabel), /*#__PURE__*/React.createElement("button", {
    onClick: onClick,
    disabled: disabled,
    style: {
      flex: secondary ? 2 : 1,
      height: 56,
      borderRadius: 9999,
      border: 0,
      background: disabled ? '#E8E8E8' : '#000',
      color: disabled ? '#959595' : '#fff',
      fontFamily: '"CashMarket","Cash Sans"',
      fontSize: 16,
      fontWeight: 500,
      cursor: disabled ? 'default' : 'pointer'
    }
  }, label));
}
function StepBody({
  title,
  sub,
  children
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '8px 24px 24px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 28,
      fontWeight: 500,
      letterSpacing: '-0.015em',
      lineHeight: 1.18,
      color: '#000',
      marginTop: 8
    }
  }, title), sub && /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 10,
      lineHeight: 1.45
    }
  }, sub), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 24
    }
  }, children));
}

// Radio-style answer option (large, tappable)
function Option({
  children,
  selected,
  onClick,
  hint
}) {
  return /*#__PURE__*/React.createElement("button", {
    onClick: onClick,
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 14,
      width: '100%',
      textAlign: 'left',
      padding: '18px 18px',
      borderRadius: 18,
      border: selected ? '2px solid #000' : '1.5px solid #E8E8E8',
      background: '#fff',
      marginBottom: 10,
      cursor: 'pointer',
      fontFamily: '"Cash Sans"'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1,
      fontSize: 16,
      fontWeight: 500,
      color: '#000'
    }
  }, children), /*#__PURE__*/React.createElement("span", {
    style: {
      width: 22,
      height: 22,
      borderRadius: 9999,
      border: selected ? '0' : '1.5px solid #D9D9D9',
      background: selected ? '#000' : 'transparent',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      flex: '0 0 22px'
    }
  }, selected && /*#__PURE__*/React.createElement("svg", {
    width: "12",
    height: "12",
    viewBox: "0 0 12 12"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M2 6l3 3 5-6",
    stroke: "#fff",
    strokeWidth: "2.2",
    fill: "none",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }))));
}

// Merchant ID card used throughout to give context
function MerchantCard({
  tx,
  expanded
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      borderRadius: 20,
      padding: 20,
      border: '1px solid #EFEFEF'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 14,
      marginBottom: expanded ? 18 : 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 52,
      height: 52,
      borderRadius: 14,
      background: '#0F2A1E',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      color: '#CCFF14',
      fontWeight: 600,
      fontSize: 20,
      letterSpacing: '-0.02em',
      flex: '0 0 52px'
    }
  }, "V"), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 17,
      fontWeight: 500,
      color: '#000',
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis'
    }
  }, tx.cleanName), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666',
      marginTop: 2
    }
  }, tx.category, " \xB7 ", tx.location)), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: 'right'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 18,
      fontWeight: 500,
      color: '#000',
      fontVariantNumeric: 'tabular-nums'
    }
  }, "$", tx.amount.toFixed(2)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: '#959595',
      marginTop: 2
    }
  }, tx.time))), expanded && /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: '1fr 1fr',
      gap: 14,
      padding: '14px 0',
      borderTop: '1px solid #F2F2F2'
    }
  }, /*#__PURE__*/React.createElement(Meta, {
    label: "Statement name",
    value: tx.merchant,
    mono: true
  }), /*#__PURE__*/React.createElement(Meta, {
    label: "Card",
    value: tx.card,
    mono: true
  }), /*#__PURE__*/React.createElement(Meta, {
    label: "Date",
    value: tx.date
  }), /*#__PURE__*/React.createElement(Meta, {
    label: "Posted",
    value: "Apr 15, 8:42 AM"
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: '1px solid #F2F2F2',
      paddingTop: 14
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: '#666',
      textTransform: 'uppercase',
      letterSpacing: '0.06em',
      fontWeight: 500,
      marginBottom: 10
    }
  }, "Map"), /*#__PURE__*/React.createElement(FakeMap, null))));
}
function Meta({
  label,
  value,
  mono
}) {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      color: '#959595',
      textTransform: 'uppercase',
      letterSpacing: '0.06em',
      fontWeight: 500,
      marginBottom: 3
    }
  }, label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 14,
      fontWeight: 500,
      color: '#000',
      fontFamily: mono ? '"Cash Sans Mono"' : '"Cash Sans"'
    }
  }, value));
}
function FakeMap() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      height: 120,
      borderRadius: 14,
      background: 'linear-gradient(135deg, #E8EBE5 0%, #D9DED1 100%)',
      position: 'relative',
      overflow: 'hidden'
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "100%",
    height: "100%",
    viewBox: "0 0 320 120",
    style: {
      position: 'absolute',
      inset: 0
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M0 40 Q80 50 160 30 T320 50",
    stroke: "#fff",
    strokeWidth: "6",
    fill: "none",
    opacity: "0.7"
  }), /*#__PURE__*/React.createElement("path", {
    d: "M0 70 L120 90 L200 70 L320 80",
    stroke: "#fff",
    strokeWidth: "3",
    fill: "none",
    opacity: "0.5"
  }), /*#__PURE__*/React.createElement("rect", {
    x: "40",
    y: "20",
    width: "22",
    height: "22",
    rx: "3",
    fill: "#fff",
    opacity: "0.4"
  }), /*#__PURE__*/React.createElement("rect", {
    x: "220",
    y: "80",
    width: "40",
    height: "22",
    rx: "3",
    fill: "#fff",
    opacity: "0.4"
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%,-100%)'
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "36",
    height: "46",
    viewBox: "0 0 36 46"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M18 0C8 0 0 8 0 18c0 13 18 28 18 28s18-15 18-28C36 8 28 0 18 0z",
    fill: "#000"
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "18",
    cy: "18",
    r: "6",
    fill: "#CCFF14"
  }))));
}

// ─── 1. Transaction detail (entry point) ───
function ScreenTransaction({
  onNext,
  setIdx
}) {
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "Transaction",
    onClose: () => {}
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '8px 24px 24px',
      background: '#F5F5F5'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: 6,
      padding: '24px 0 12px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 64,
      height: 64,
      borderRadius: 18,
      background: '#0F2A1E',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      color: '#CCFF14',
      fontWeight: 600,
      fontSize: 26
    }
  }, "V"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666',
      marginTop: 8
    }
  }, "Card payment"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'baseline',
      gap: 1
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 32,
      color: '#959595',
      fontFamily: '"Cash Sans"'
    }
  }, "$"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 64,
      fontWeight: 400,
      fontFamily: '"Cash Sans"',
      letterSpacing: '-0.02em',
      lineHeight: 1
    }
  }, "5"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 32,
      color: '#959595',
      fontFamily: '"Cash Sans"'
    }
  }, ".42")), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 2
    }
  }, TX.cleanName)), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      borderRadius: 20,
      marginTop: 24,
      overflow: 'hidden'
    }
  }, /*#__PURE__*/React.createElement(Row, {
    icon: "time",
    label: "Date",
    detail: "Apr 15, 2026 \xB7 8:42 AM"
  }), /*#__PURE__*/React.createElement(Row, {
    icon: "cardBasic",
    label: "Card",
    detail: "Cash Card \xB7\xB7\xB7\xB7 4317"
  }), /*#__PURE__*/React.createElement(Row, {
    icon: "location",
    label: "Location",
    detail: "Santa Cruz, CA"
  }), /*#__PURE__*/React.createElement(Row, {
    icon: "note",
    label: "Statement name",
    detail: "SQ *Verve Coffee",
    isLast: true
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      borderRadius: 20,
      marginTop: 14,
      overflow: 'hidden'
    }
  }, /*#__PURE__*/React.createElement(RowTap, {
    icon: "alert",
    label: "Report an issue",
    onClick: onNext
  }), /*#__PURE__*/React.createElement(RowTap, {
    icon: "commChat",
    label: "Get help",
    isLast: true
  }))));
}
function Row({
  icon,
  label,
  detail,
  isLast
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 12,
      padding: '14px 18px',
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: `../../assets/icons/${icon}24.svg`,
    width: 20,
    height: 20,
    style: {
      opacity: 0.65
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      fontSize: 15,
      color: '#666'
    }
  }, label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      fontWeight: 500,
      color: '#000',
      fontFamily: typeof detail === 'string' && detail.includes('····') ? '"Cash Sans Mono"' : '"Cash Sans"'
    }
  }, detail), !isLast && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: 50,
      right: 0,
      bottom: 0,
      height: '0.5px',
      background: '#F0F0F0'
    }
  }));
}
function RowTap({
  icon,
  label,
  onClick,
  isLast
}) {
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 12,
      padding: '16px 18px',
      position: 'relative',
      cursor: 'pointer'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: `../../assets/icons/${icon}24.svg`,
    width: 22,
    height: 22
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      fontSize: 16,
      fontWeight: 500,
      color: '#000'
    }
  }, label), /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/next24.svg",
    width: 18,
    height: 18,
    style: {
      opacity: 0.4
    }
  }), !isLast && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: 50,
      right: 0,
      bottom: 0,
      height: '0.5px',
      background: '#F0F0F0'
    }
  }));
}

// ─── 2. Do you recognize this transaction? ───
function ScreenRecognize({
  onNext,
  onBack,
  setPath
}) {
  const [a, setA] = React.useState(null);
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement(StepBody, {
    title: "Do you recognize this transaction?",
    sub: "We'll use your answer to figure out what kind of dispute this is."
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 18
    }
  }, /*#__PURE__*/React.createElement(MerchantCard, {
    tx: TX
  })), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'yes',
    onClick: () => setA('yes')
  }, "Yes, I recognize it"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'maybe',
    onClick: () => setA('maybe')
  }, "I'm not sure"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'no',
    onClick: () => setA('no')
  }, "No, I don't recognize it")), /*#__PURE__*/React.createElement(ContinueBar, {
    disabled: !a,
    onClick: () => {
      if (a === 'yes') {
        setPath('service');
      } else if (a === 'maybe') {/* show merchant context */} else {
        setPath('unrecognized');
      }
      onNext();
    }
  }));
}

// ─── 3. Merchant context (jog the memory) ───
function ScreenMerchant({
  onNext,
  onBack
}) {
  const [a, setA] = React.useState(null);
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement(StepBody, {
    title: "Does this jog your memory?",
    sub: "Sometimes charges show up under a different name than the place where you paid."
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 14
    }
  }, /*#__PURE__*/React.createElement(MerchantCard, {
    tx: TX,
    expanded: true
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#F2F2F2',
      borderRadius: 16,
      padding: 14,
      display: 'flex',
      gap: 12,
      marginBottom: 18
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/information24.svg",
    width: 20,
    height: 20,
    style: {
      opacity: 0.7,
      marginTop: 1,
      flex: '0 0 20px'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#333',
      lineHeight: 1.45
    }
  }, "You've paid ", /*#__PURE__*/React.createElement("strong", null, TX.cleanName), " ", TX.priorCount, " times in the last 90 days. It often shows up as ", /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: '"Cash Sans Mono"'
    }
  }, "SQ *Verve Coffee"), ".")), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'yes',
    onClick: () => setA('yes')
  }, "Yes \u2014 I remember now"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'no',
    onClick: () => setA('no')
  }, "No, still don't recognize it")), /*#__PURE__*/React.createElement(ContinueBar, {
    disabled: !a,
    onClick: onNext
  }));
}

// ─── 4. Amount check ───
function ScreenAmountCheck({
  onNext,
  onBack
}) {
  const [a, setA] = React.useState(null);
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement(StepBody, {
    title: "Was this the amount you expected?",
    sub: "We'll check whether the charge matches what you remember."
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      borderRadius: 20,
      padding: '28px 20px',
      border: '1px solid #EFEFEF',
      marginBottom: 18,
      textAlign: 'center'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: '#959595',
      textTransform: 'uppercase',
      letterSpacing: '0.06em',
      marginBottom: 6
    }
  }, "You were charged"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'baseline',
      justifyContent: 'center',
      gap: 1
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 24,
      color: '#959595'
    }
  }, "$"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 56,
      fontWeight: 400,
      fontFamily: '"Cash Sans"',
      letterSpacing: '-0.02em',
      lineHeight: 1
    }
  }, "5"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 24,
      color: '#959595'
    }
  }, ".42"))), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'match',
    onClick: () => setA('match')
  }, "Yes, this matches"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'higher',
    onClick: () => setA('higher')
  }, "It's more than I expected"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'duplicate',
    onClick: () => setA('duplicate')
  }, "I was charged more than once"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'refunded',
    onClick: () => setA('refunded')
  }, "I was supposed to get a refund")), /*#__PURE__*/React.createElement(ContinueBar, {
    disabled: !a,
    onClick: onNext
  }));
}

// ─── 5. Service / product check ───
function ScreenService({
  onNext,
  onBack
}) {
  const [a, setA] = React.useState(null);
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement(StepBody, {
    title: "Did you get what you paid for?"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 18
    }
  }, /*#__PURE__*/React.createElement(MerchantCard, {
    tx: TX
  })), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'yes',
    onClick: () => setA('yes')
  }, "Yes"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'notReceived',
    onClick: () => setA('notReceived')
  }, "No \u2014 I never received it"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'wrongItem',
    onClick: () => setA('wrongItem')
  }, "It wasn't what was described"), /*#__PURE__*/React.createElement(Option, {
    selected: a === 'cancelled',
    onClick: () => setA('cancelled')
  }, "I cancelled but was still charged")), /*#__PURE__*/React.createElement(ContinueBar, {
    disabled: !a,
    onClick: onNext
  }));
}

// ─── 6. Inferred issue confirmation ───
function ScreenInferred({
  onNext,
  onBack,
  path
}) {
  const inferred = path === 'unrecognized' ? {
    tag: 'Unauthorized charge',
    desc: "You don't recognize this transaction. We'll treat it as a possible unauthorized charge and protect your account.",
    illo: 'lock-portal.svg'
  } : path === 'service' ? {
    tag: "Didn't receive service",
    desc: "You paid but didn't get what you expected. We'll request a chargeback from the merchant.",
    illo: 'safety-glass.svg'
  } : {
    tag: 'Wrong amount',
    desc: "The amount charged doesn't match what you agreed to. We'll dispute the difference.",
    illo: 'calculator.svg'
  };
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '8px 24px 24px',
      display: 'flex',
      flexDirection: 'column'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      padding: '16px 0 8px'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: `../../assets/illustrations/${inferred.illo}`,
    style: {
      width: 120,
      height: 120,
      objectFit: 'contain'
    },
    onError: e => {
      e.target.style.display = 'none';
    }
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: 'center',
      marginTop: 8
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      fontWeight: 500,
      color: '#666',
      textTransform: 'uppercase',
      letterSpacing: '0.08em',
      marginBottom: 10
    }
  }, "Based on your answers"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 28,
      fontWeight: 500,
      letterSpacing: '-0.015em',
      lineHeight: 1.18
    }
  }, inferred.tag), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 12,
      lineHeight: 1.45,
      padding: '0 8px'
    }
  }, inferred.desc)), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 'auto',
      paddingTop: 24
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#F2F2F2',
      borderRadius: 16,
      padding: 14,
      display: 'flex',
      gap: 12,
      marginBottom: 6
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/edit24.svg",
    width: 20,
    height: 20,
    style: {
      opacity: 0.7,
      flex: '0 0 20px',
      marginTop: 1
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 14,
      fontWeight: 500,
      color: '#000'
    }
  }, "Doesn't sound right?"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666',
      marginTop: 2
    }
  }, "Tap to pick a different reason")), /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/next24.svg",
    width: 18,
    height: 18,
    style: {
      opacity: 0.4,
      alignSelf: 'center'
    }
  })))), /*#__PURE__*/React.createElement(ContinueBar, {
    label: "Looks right, continue",
    onClick: onNext,
    secondary: true,
    onSecondary: onBack
  }));
}

// ─── 7. Card cancel warning ───
function ScreenCardCancel({
  onNext,
  onBack,
  path
}) {
  if (path !== 'unrecognized') {
    // skip card cancellation if not unauthorized
    return /*#__PURE__*/React.createElement(ScreenEvidence, {
      onNext: onNext,
      onBack: onBack
    });
  }
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '8px 24px 24px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      padding: '24px 0 12px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 120,
      height: 80,
      borderRadius: 12,
      background: 'linear-gradient(135deg,#CCFF14 0%, #00E013 100%)',
      position: 'relative',
      transform: 'rotate(-6deg)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      background: 'rgba(0,0,0,0.8)',
      borderRadius: 12
    }
  }), /*#__PURE__*/React.createElement("svg", {
    width: "44",
    height: "44",
    viewBox: "0 0 24 24",
    style: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      transform: 'translate(-50%,-50%)'
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M3 3l18 18M5 12a7 7 0 0114-1",
    stroke: "#fff",
    strokeWidth: "2",
    fill: "none",
    strokeLinecap: "round"
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 28,
      fontWeight: 500,
      letterSpacing: '-0.015em',
      lineHeight: 1.18,
      marginTop: 12
    }
  }, "Your Cash Card will be canceled"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 12,
      lineHeight: 1.5
    }
  }, "For your protection, we'll cancel ", /*#__PURE__*/React.createElement("strong", {
    style: {
      color: '#000'
    }
  }, "Cash Card \xB7\xB7\xB7\xB7 4317"), " to prevent additional unauthorized charges."), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 14,
      lineHeight: 1.5
    }
  }, "You'll be able to use a virtual replacement right away. A physical card arrives in about 10 days."), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#FFF6E5',
      borderRadius: 16,
      padding: 14,
      display: 'flex',
      gap: 12,
      marginTop: 18,
      border: '1px solid #F5E2B3'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/information24.svg",
    width: 20,
    height: 20,
    style: {
      opacity: 0.7,
      flex: '0 0 20px',
      marginTop: 1
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      fontSize: 13,
      color: '#5C4500',
      lineHeight: 1.5
    }
  }, "Existing subscriptions will keep working. We've blocked any new charges."))), /*#__PURE__*/React.createElement(ContinueBar, {
    label: "Continue",
    onClick: onNext,
    secondary: true,
    onSecondary: onBack
  }));
}

// ─── 8. Evidence + note ───
function ScreenEvidence({
  onNext,
  onBack
}) {
  const [note, setNote] = React.useState('');
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement(StepBody, {
    title: "Anything else we should know?",
    sub: "Add a note or attach a receipt, email, or screenshot. This is optional but speeds up the review."
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      border: '1.5px solid #E8E8E8',
      borderRadius: 18,
      padding: 16,
      marginBottom: 12
    }
  }, /*#__PURE__*/React.createElement("textarea", {
    value: note,
    onChange: e => setNote(e.target.value),
    placeholder: "e.g. I was traveling that day, I never visited Santa Cruz\u2026",
    style: {
      width: '100%',
      minHeight: 100,
      border: 0,
      outline: 0,
      resize: 'none',
      fontFamily: '"Cash Sans"',
      fontSize: 15,
      color: '#000',
      background: 'transparent',
      lineHeight: 1.45
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: 'right',
      fontFamily: '"Cash Sans Mono"',
      fontSize: 11,
      color: '#959595'
    }
  }, note.length, "/5000")), /*#__PURE__*/React.createElement("button", {
    style: {
      display: 'flex',
      width: '100%',
      padding: '18px',
      alignItems: 'center',
      gap: 12,
      background: '#fff',
      border: '1.5px dashed #D9D9D9',
      borderRadius: 18,
      cursor: 'pointer',
      fontFamily: '"Cash Sans"'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/add24.svg",
    width: 20,
    height: 20
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1,
      textAlign: 'left',
      fontSize: 15,
      fontWeight: 500
    }
  }, "Attach a file"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 12,
      color: '#959595'
    }
  }, "JPG, PNG, PDF"))), /*#__PURE__*/React.createElement(ContinueBar, {
    label: "Continue",
    onClick: onNext,
    secondary: true,
    onSecondary: onBack
  }));
}

// ─── 9. Review & submit ───
function ScreenReview({
  onNext,
  onBack,
  path
}) {
  const issueLabel = path === 'unrecognized' ? 'Unauthorized charge' : path === 'service' ? "Didn't receive service" : 'Wrong amount';
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(NavBar, {
    title: "",
    onBack: onBack,
    onClose: () => {}
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '8px 24px 24px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 28,
      fontWeight: 500,
      letterSpacing: '-0.015em',
      lineHeight: 1.18,
      marginTop: 8
    }
  }, "Ready to submit?"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 10,
      lineHeight: 1.45
    }
  }, "We'll investigate using what you told us and get back to you within 10 business days."), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 22,
      marginBottom: 14
    }
  }, /*#__PURE__*/React.createElement(MerchantCard, {
    tx: TX
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      borderRadius: 20,
      padding: 18,
      border: '1px solid #EFEFEF'
    }
  }, /*#__PURE__*/React.createElement(SummaryRow, {
    label: "Dispute type",
    value: issueLabel
  }), /*#__PURE__*/React.createElement(SummaryRow, {
    label: "Card",
    value: "\xB7\xB7\xB7\xB7 4317",
    mono: true
  }), /*#__PURE__*/React.createElement(SummaryRow, {
    label: "Action",
    value: path === 'unrecognized' ? 'Cancel card + issue replacement' : 'Request chargeback'
  }), /*#__PURE__*/React.createElement(SummaryRow, {
    label: "Contact",
    value: "alex@example.com",
    last: true
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: '#959595',
      marginTop: 16,
      lineHeight: 1.5,
      padding: '0 4px'
    }
  }, "By submitting, I confirm the information provided is accurate. Cash App can't guarantee the funds will be returned.")), /*#__PURE__*/React.createElement(ContinueBar, {
    label: "Submit",
    onClick: onNext,
    secondary: true,
    onSecondary: onBack
  }));
}
function SummaryRow({
  label,
  value,
  mono,
  last
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      padding: '12px 0',
      borderBottom: last ? 0 : '1px solid #F2F2F2'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      fontSize: 14,
      color: '#666'
    }
  }, label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 14,
      fontWeight: 500,
      color: '#000',
      fontFamily: mono ? '"Cash Sans Mono"' : '"Cash Sans"',
      textAlign: 'right'
    }
  }, value));
}

// ─── 10. Submitted ───
function ScreenSubmitted({
  setIdx
}) {
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '40px 28px 24px',
      display: 'flex',
      flexDirection: 'column'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      padding: '16px 0 8px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 80,
      height: 80,
      borderRadius: 9999,
      background: '#00D64F',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center'
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "40",
    height: "40",
    viewBox: "0 0 24 24"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M5 12l4 4 10-10",
    stroke: "#fff",
    strokeWidth: "3",
    fill: "none",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: 'center',
      marginTop: 18
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 30,
      fontWeight: 500,
      letterSpacing: '-0.015em',
      lineHeight: 1.15
    }
  }, "Dispute submitted"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      color: '#666',
      marginTop: 12,
      lineHeight: 1.5,
      padding: '0 8px'
    }
  }, "We'll review your dispute and get back to you within 10 business days.")), /*#__PURE__*/React.createElement("div", {
    style: {
      background: '#fff',
      borderRadius: 20,
      marginTop: 28,
      padding: '4px 0',
      border: '1px solid #EFEFEF'
    }
  }, /*#__PURE__*/React.createElement(TimelineRow, {
    icon: "check",
    title: "Dispute received",
    sub: "Today",
    done: true
  }), /*#__PURE__*/React.createElement(TimelineRow, {
    icon: "time",
    title: "Investigation",
    sub: "1\u201310 business days"
  }), /*#__PURE__*/React.createElement(TimelineRow, {
    icon: "commChat",
    title: "We'll notify you",
    sub: "Via push and email",
    last: true
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 14,
      background: '#F2F2F2',
      borderRadius: 16,
      padding: 14,
      display: 'flex',
      gap: 12,
      alignItems: 'center'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/cardBasic24.svg",
    width: 22,
    height: 22
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 14,
      fontWeight: 500
    }
  }, "Replacement card on the way"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666',
      marginTop: 1
    }
  }, "Arrives in ~10 days \xB7 Use virtually now")), /*#__PURE__*/React.createElement("img", {
    src: "../../assets/icons/next24.svg",
    width: 18,
    height: 18,
    style: {
      opacity: 0.5
    }
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '12px 16px 24px',
      background: '#F5F5F5'
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: () => setIdx(0),
    style: {
      width: '100%',
      height: 56,
      borderRadius: 9999,
      border: 0,
      background: '#000',
      color: '#fff',
      fontFamily: '"CashMarket","Cash Sans"',
      fontSize: 16,
      fontWeight: 500,
      cursor: 'pointer'
    }
  }, "Done")));
}
function TimelineRow({
  icon,
  title,
  sub,
  done,
  last
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 14,
      padding: '14px 18px',
      alignItems: 'center',
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 36,
      height: 36,
      borderRadius: 9999,
      background: done ? '#000' : '#F2F2F2',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      flex: '0 0 36px'
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: `../../assets/icons/${icon}24.svg`,
    width: 18,
    height: 18,
    style: {
      filter: done ? 'invert(1)' : 'none'
    }
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 15,
      fontWeight: 500,
      color: '#000'
    }
  }, title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      color: '#666',
      marginTop: 1
    }
  }, sub)), !last && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: 52,
      bottom: 0,
      right: 0,
      height: '0.5px',
      background: '#F0F0F0'
    }
  }));
}
Object.assign(window, {
  ScreenTransaction,
  ScreenRecognize,
  ScreenMerchant,
  ScreenAmountCheck,
  ScreenService,
  ScreenInferred,
  ScreenCardCancel,
  ScreenEvidence,
  ScreenReview,
  ScreenSubmitted
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/mobile/dispute-screens.jsx", error: String((e && e.message) || e) }); }

// ui_kits/mobile/ios-frame.jsx
try { (() => {
// iOS.jsx — Simplified iOS 26 (Liquid Glass) device frame
// Based on the iOS 26 UI Kit + Figma status bar spec. No assets, no deps.
// Exports: IOSDevice, IOSStatusBar, IOSNavBar, IOSGlassPill, IOSList, IOSListRow, IOSKeyboard

// ─────────────────────────────────────────────────────────────
// Status bar
// ─────────────────────────────────────────────────────────────
function IOSStatusBar({
  dark = false,
  time = '9:41'
}) {
  const c = dark ? '#fff' : '#000';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 154,
      alignItems: 'center',
      justifyContent: 'center',
      padding: '21px 24px 19px',
      boxSizing: 'border-box',
      position: 'relative',
      zIndex: 20,
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 22,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      paddingTop: 1.5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: '-apple-system, "SF Pro", system-ui',
      fontWeight: 590,
      fontSize: 17,
      lineHeight: '22px',
      color: c
    }
  }, time)), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 22,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 7,
      paddingTop: 1,
      paddingRight: 1
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "19",
    height: "12",
    viewBox: "0 0 19 12"
  }, /*#__PURE__*/React.createElement("rect", {
    x: "0",
    y: "7.5",
    width: "3.2",
    height: "4.5",
    rx: "0.7",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "4.8",
    y: "5",
    width: "3.2",
    height: "7",
    rx: "0.7",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "9.6",
    y: "2.5",
    width: "3.2",
    height: "9.5",
    rx: "0.7",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "14.4",
    y: "0",
    width: "3.2",
    height: "12",
    rx: "0.7",
    fill: c
  })), /*#__PURE__*/React.createElement("svg", {
    width: "17",
    height: "12",
    viewBox: "0 0 17 12"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M8.5 3.2C10.8 3.2 12.9 4.1 14.4 5.6L15.5 4.5C13.7 2.7 11.2 1.5 8.5 1.5C5.8 1.5 3.3 2.7 1.5 4.5L2.6 5.6C4.1 4.1 6.2 3.2 8.5 3.2Z",
    fill: c
  }), /*#__PURE__*/React.createElement("path", {
    d: "M8.5 6.8C9.9 6.8 11.1 7.3 12 8.2L13.1 7.1C11.8 5.9 10.2 5.1 8.5 5.1C6.8 5.1 5.2 5.9 3.9 7.1L5 8.2C5.9 7.3 7.1 6.8 8.5 6.8Z",
    fill: c
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "8.5",
    cy: "10.5",
    r: "1.5",
    fill: c
  })), /*#__PURE__*/React.createElement("svg", {
    width: "27",
    height: "13",
    viewBox: "0 0 27 13"
  }, /*#__PURE__*/React.createElement("rect", {
    x: "0.5",
    y: "0.5",
    width: "23",
    height: "12",
    rx: "3.5",
    stroke: c,
    strokeOpacity: "0.35",
    fill: "none"
  }), /*#__PURE__*/React.createElement("rect", {
    x: "2",
    y: "2",
    width: "20",
    height: "9",
    rx: "2",
    fill: c
  }), /*#__PURE__*/React.createElement("path", {
    d: "M25 4.5V8.5C25.8 8.2 26.5 7.2 26.5 6.5C26.5 5.8 25.8 4.8 25 4.5Z",
    fill: c,
    fillOpacity: "0.4"
  }))));
}

// ─────────────────────────────────────────────────────────────
// Liquid glass pill — blur + tint + shine
// ─────────────────────────────────────────────────────────────
function IOSGlassPill({
  children,
  dark = false,
  style = {}
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      height: 44,
      minWidth: 44,
      borderRadius: 9999,
      position: 'relative',
      overflow: 'hidden',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      boxShadow: dark ? '0 2px 6px rgba(0,0,0,0.35), 0 6px 16px rgba(0,0,0,0.2)' : '0 1px 3px rgba(0,0,0,0.07), 0 3px 10px rgba(0,0,0,0.06)',
      ...style
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 9999,
      backdropFilter: 'blur(12px) saturate(180%)',
      WebkitBackdropFilter: 'blur(12px) saturate(180%)',
      background: dark ? 'rgba(120,120,128,0.28)' : 'rgba(255,255,255,0.5)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 9999,
      boxShadow: dark ? 'inset 1.5px 1.5px 1px rgba(255,255,255,0.15), inset -1px -1px 1px rgba(255,255,255,0.08)' : 'inset 1.5px 1.5px 1px rgba(255,255,255,0.7), inset -1px -1px 1px rgba(255,255,255,0.4)',
      border: dark ? '0.5px solid rgba(255,255,255,0.15)' : '0.5px solid rgba(0,0,0,0.06)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      zIndex: 1,
      display: 'flex',
      alignItems: 'center',
      padding: '0 4px'
    }
  }, children));
}

// ─────────────────────────────────────────────────────────────
// Navigation bar — glass pills + large title
// ─────────────────────────────────────────────────────────────
function IOSNavBar({
  title = 'Title',
  dark = false,
  trailingIcon = true
}) {
  const muted = dark ? 'rgba(255,255,255,0.6)' : '#404040';
  const text = dark ? '#fff' : '#000';
  const pillIcon = content => /*#__PURE__*/React.createElement(IOSGlassPill, {
    dark: dark
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 36,
      height: 36,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center'
    }
  }, content));
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 10,
      paddingTop: 62,
      paddingBottom: 10,
      position: 'relative',
      zIndex: 5
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      padding: '0 16px'
    }
  }, pillIcon(/*#__PURE__*/React.createElement("svg", {
    width: "12",
    height: "20",
    viewBox: "0 0 12 20",
    fill: "none",
    style: {
      marginLeft: -1
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M10 2L2 10l8 8",
    stroke: muted,
    strokeWidth: "2.5",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }))), trailingIcon && pillIcon(/*#__PURE__*/React.createElement("svg", {
    width: "22",
    height: "6",
    viewBox: "0 0 22 6"
  }, /*#__PURE__*/React.createElement("circle", {
    cx: "3",
    cy: "3",
    r: "2.5",
    fill: muted
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "11",
    cy: "3",
    r: "2.5",
    fill: muted
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "19",
    cy: "3",
    r: "2.5",
    fill: muted
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '0 16px',
      fontFamily: '-apple-system, system-ui',
      fontSize: 34,
      fontWeight: 700,
      lineHeight: '41px',
      color: text,
      letterSpacing: 0.4
    }
  }, title));
}

// ─────────────────────────────────────────────────────────────
// Grouped list (inset card, r:26) + row (52px)
// ─────────────────────────────────────────────────────────────
function IOSListRow({
  title,
  detail,
  icon,
  chevron = true,
  isLast = false,
  dark = false
}) {
  const text = dark ? '#fff' : '#000';
  const sec = dark ? 'rgba(235,235,245,0.6)' : 'rgba(60,60,67,0.6)';
  const ter = dark ? 'rgba(235,235,245,0.3)' : 'rgba(60,60,67,0.3)';
  const sep = dark ? 'rgba(84,84,88,0.65)' : 'rgba(60,60,67,0.12)';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      minHeight: 52,
      padding: '0 16px',
      position: 'relative',
      fontFamily: '-apple-system, system-ui',
      fontSize: 17,
      letterSpacing: -0.43
    }
  }, icon && /*#__PURE__*/React.createElement("div", {
    style: {
      width: 30,
      height: 30,
      borderRadius: 7,
      background: icon,
      marginRight: 12,
      flexShrink: 0
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      color: text
    }
  }, title), detail && /*#__PURE__*/React.createElement("span", {
    style: {
      color: sec,
      marginRight: 6
    }
  }, detail), chevron && /*#__PURE__*/React.createElement("svg", {
    width: "8",
    height: "14",
    viewBox: "0 0 8 14",
    style: {
      flexShrink: 0
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M1 1l6 6-6 6",
    stroke: ter,
    strokeWidth: "2",
    fill: "none",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  })), !isLast && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      bottom: 0,
      right: 0,
      left: icon ? 58 : 16,
      height: 0.5,
      background: sep
    }
  }));
}
function IOSList({
  header,
  children,
  dark = false
}) {
  const hc = dark ? 'rgba(235,235,245,0.6)' : 'rgba(60,60,67,0.6)';
  const bg = dark ? '#1C1C1E' : '#fff';
  return /*#__PURE__*/React.createElement("div", null, header && /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: '-apple-system, system-ui',
      fontSize: 13,
      color: hc,
      textTransform: 'uppercase',
      padding: '8px 36px 6px',
      letterSpacing: -0.08
    }
  }, header), /*#__PURE__*/React.createElement("div", {
    style: {
      background: bg,
      borderRadius: 26,
      margin: '0 16px',
      overflow: 'hidden'
    }
  }, children));
}

// ─────────────────────────────────────────────────────────────
// Device frame
// ─────────────────────────────────────────────────────────────
function IOSDevice({
  children,
  width = 402,
  height = 874,
  dark = false,
  title,
  keyboard = false
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width,
      height,
      borderRadius: 48,
      overflow: 'hidden',
      position: 'relative',
      background: dark ? '#000' : '#F2F2F7',
      boxShadow: '0 40px 80px rgba(0,0,0,0.18), 0 0 0 1px rgba(0,0,0,0.12)',
      fontFamily: '-apple-system, system-ui, sans-serif',
      WebkitFontSmoothing: 'antialiased'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: 11,
      left: '50%',
      transform: 'translateX(-50%)',
      width: 126,
      height: 37,
      borderRadius: 24,
      background: '#000',
      zIndex: 50
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      zIndex: 10
    }
  }, /*#__PURE__*/React.createElement(IOSStatusBar, {
    dark: dark
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      height: '100%',
      display: 'flex',
      flexDirection: 'column'
    }
  }, title !== undefined && /*#__PURE__*/React.createElement(IOSNavBar, {
    title: title,
    dark: dark
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto'
    }
  }, children), keyboard && /*#__PURE__*/React.createElement(IOSKeyboard, {
    dark: dark
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      bottom: 0,
      left: 0,
      right: 0,
      zIndex: 60,
      height: 34,
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'flex-end',
      paddingBottom: 8,
      pointerEvents: 'none'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 139,
      height: 5,
      borderRadius: 100,
      background: dark ? 'rgba(255,255,255,0.7)' : 'rgba(0,0,0,0.25)'
    }
  })));
}

// ─────────────────────────────────────────────────────────────
// Keyboard — iOS 26 liquid glass
// ─────────────────────────────────────────────────────────────
function IOSKeyboard({
  dark = false
}) {
  const glyph = dark ? 'rgba(255,255,255,0.7)' : '#595959';
  const sugg = dark ? 'rgba(255,255,255,0.6)' : '#333';
  const keyBg = dark ? 'rgba(255,255,255,0.22)' : 'rgba(255,255,255,0.85)';

  // special-key icons
  const icons = {
    shift: /*#__PURE__*/React.createElement("svg", {
      width: "19",
      height: "17",
      viewBox: "0 0 19 17"
    }, /*#__PURE__*/React.createElement("path", {
      d: "M9.5 1L1 9.5h4.5V16h8V9.5H18L9.5 1z",
      fill: glyph
    })),
    del: /*#__PURE__*/React.createElement("svg", {
      width: "23",
      height: "17",
      viewBox: "0 0 23 17"
    }, /*#__PURE__*/React.createElement("path", {
      d: "M7 1h13a2 2 0 012 2v11a2 2 0 01-2 2H7l-6-7.5L7 1z",
      fill: "none",
      stroke: glyph,
      strokeWidth: "1.6",
      strokeLinejoin: "round"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M10 5l7 7M17 5l-7 7",
      stroke: glyph,
      strokeWidth: "1.6",
      strokeLinecap: "round"
    })),
    ret: /*#__PURE__*/React.createElement("svg", {
      width: "20",
      height: "14",
      viewBox: "0 0 20 14"
    }, /*#__PURE__*/React.createElement("path", {
      d: "M18 1v6H4m0 0l4-4M4 7l4 4",
      fill: "none",
      stroke: "#fff",
      strokeWidth: "1.8",
      strokeLinecap: "round",
      strokeLinejoin: "round"
    }))
  };
  const key = (content, {
    w,
    flex,
    ret,
    fs = 25,
    k
  } = {}) => /*#__PURE__*/React.createElement("div", {
    key: k,
    style: {
      height: 42,
      borderRadius: 8.5,
      flex: flex ? 1 : undefined,
      width: w,
      minWidth: 0,
      background: ret ? '#08f' : keyBg,
      boxShadow: '0 1px 0 rgba(0,0,0,0.075)',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      fontFamily: '-apple-system, "SF Compact", system-ui',
      fontSize: fs,
      fontWeight: 458,
      color: ret ? '#fff' : glyph
    }
  }, content);
  const row = (keys, pad = 0) => /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6.5,
      justifyContent: 'center',
      padding: `0 ${pad}px`
    }
  }, keys.map(l => key(l, {
    flex: true,
    k: l
  })));
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      zIndex: 15,
      borderRadius: 27,
      overflow: 'hidden',
      padding: '11px 0 2px',
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      boxShadow: dark ? '0 -2px 20px rgba(0,0,0,0.09)' : '0 -1px 6px rgba(0,0,0,0.018), 0 -3px 20px rgba(0,0,0,0.012)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 27,
      backdropFilter: 'blur(12px) saturate(180%)',
      WebkitBackdropFilter: 'blur(12px) saturate(180%)',
      background: dark ? 'rgba(120,120,128,0.14)' : 'rgba(255,255,255,0.25)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 27,
      boxShadow: dark ? 'inset 1.5px 1.5px 1px rgba(255,255,255,0.15)' : 'inset 1.5px 1.5px 1px rgba(255,255,255,0.7), inset -1px -1px 1px rgba(255,255,255,0.4)',
      border: dark ? '0.5px solid rgba(255,255,255,0.15)' : '0.5px solid rgba(0,0,0,0.06)',
      pointerEvents: 'none'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 20,
      alignItems: 'center',
      padding: '8px 22px 13px',
      width: '100%',
      boxSizing: 'border-box',
      position: 'relative'
    }
  }, ['"The"', 'the', 'to'].map((w, i) => /*#__PURE__*/React.createElement(React.Fragment, {
    key: i
  }, i > 0 && /*#__PURE__*/React.createElement("div", {
    style: {
      width: 1,
      height: 25,
      background: '#ccc',
      opacity: 0.3
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      textAlign: 'center',
      fontFamily: '-apple-system, system-ui',
      fontSize: 17,
      color: sugg,
      letterSpacing: -0.43,
      lineHeight: '22px'
    }
  }, w)))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 13,
      padding: '0 6.5px',
      width: '100%',
      boxSizing: 'border-box',
      position: 'relative'
    }
  }, row(['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p']), row(['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'], 20), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 14.25,
      alignItems: 'center'
    }
  }, key(icons.shift, {
    w: 45,
    k: 'shift'
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6.5,
      flex: 1
    }
  }, ['z', 'x', 'c', 'v', 'b', 'n', 'm'].map(l => key(l, {
    flex: true,
    k: l
  }))), key(icons.del, {
    w: 45,
    k: 'del'
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6,
      alignItems: 'center'
    }
  }, key('ABC', {
    w: 92.25,
    fs: 18,
    k: 'abc'
  }), key('', {
    flex: true,
    k: 'space'
  }), key(icons.ret, {
    w: 92.25,
    ret: true,
    k: 'ret'
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      height: 56,
      width: '100%',
      position: 'relative'
    }
  }));
}
Object.assign(window, {
  IOSDevice,
  IOSStatusBar,
  IOSNavBar,
  IOSGlassPill,
  IOSList,
  IOSListRow,
  IOSKeyboard
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/mobile/ios-frame.jsx", error: String((e && e.message) || e) }); }

})();
