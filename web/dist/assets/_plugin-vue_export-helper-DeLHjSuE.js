import{m as bo,c as P,E as Eo,a as X,am as Bo,ak as xo,d as Do,b as R,s as v,L as lo,K as Ro,n as ao,aU as Go,M as Wo,P as ko,N as Oo,R as Mo,u as _o,e as fo,B as jo,g as No,x as Ko,i as t,J as Y}from"./index-DpCLdR35.js";import{f as vo,i as ho,c as N,p as Lo,d as po,h as I,t as Vo,n as Qo,b as Q}from"./echarts-BRj3at4q.js";const Z=typeof document<"u"&&typeof window<"u";function co(o){return o.replace(/#|\(|\)|,|\s|\./g,"_")}const Co=new WeakSet;function le(o){Co.add(o)}function ae(o){return!Co.has(o)}const uo=bo("n-form-item");function qo(o,{defaultSize:x="medium",mergedSize:b,mergedDisabled:d}={}){const n=ho(uo,null);Lo(uo,null);const G=N(b?()=>b(n):()=>{const{size:S}=o;if(S)return S;if(n){const{mergedSize:W}=n;if(W.value!==void 0)return W.value}return x}),K=N(d?()=>d(n):()=>{const{disabled:S}=o;return S!==void 0?S:n?n.disabled.value:!1}),O=N(()=>{const{status:S}=o;return S||(n==null?void 0:n.mergedValidationStatus.value)});return vo(()=>{n&&n.restoreValidation()}),{mergedSizeRef:G,mergedDisabledRef:K,mergedStatusRef:O,nTriggerFormBlur(){n&&n.handleContentBlur()},nTriggerFormChange(){n&&n.handleContentChange()},nTriggerFormFocus(){n&&n.handleContentFocus()},nTriggerFormInput(){n&&n.handleContentInput()}}}const{cubicBezierEaseInOut:k}=Eo;function Ao({duration:o=".2s",delay:x=".1s"}={}){return[P("&.fade-in-width-expand-transition-leave-from, &.fade-in-width-expand-transition-enter-to",{opacity:1}),P("&.fade-in-width-expand-transition-leave-to, &.fade-in-width-expand-transition-enter-from",`
 opacity: 0!important;
 margin-left: 0!important;
 margin-right: 0!important;
 `),P("&.fade-in-width-expand-transition-leave-active",`
 overflow: hidden;
 transition:
 opacity ${o} ${k},
 max-width ${o} ${k} ${x},
 margin-left ${o} ${k} ${x},
 margin-right ${o} ${k} ${x};
 `),P("&.fade-in-width-expand-transition-enter-active",`
 overflow: hidden;
 transition:
 opacity ${o} ${k} ${x},
 max-width ${o} ${k},
 margin-left ${o} ${k},
 margin-right ${o} ${k};
 `)]}const Uo=X("base-wave",`
 position: absolute;
 left: 0;
 right: 0;
 top: 0;
 bottom: 0;
 border-radius: inherit;
`),Yo=po({name:"BaseWave",props:{clsPrefix:{type:String,required:!0}},setup(o){Bo("-base-wave",Uo,Vo(o,"clsPrefix"));const x=Q(null),b=Q(!1);let d=null;return vo(()=>{d!==null&&window.clearTimeout(d)}),{active:b,selfRef:x,play(){d!==null&&(window.clearTimeout(d),b.value=!1,d=null),Qo(()=>{var n;(n=x.value)===null||n===void 0||n.offsetHeight,b.value=!0,d=window.setTimeout(()=>{b.value=!1,d=null},1e3)})}}},render(){const{clsPrefix:o}=this;return I("div",{ref:"selfRef","aria-hidden":!0,class:[`${o}-base-wave`,this.active&&`${o}-base-wave--active`]})}}),Jo=Z&&"chrome"in window;Z&&navigator.userAgent.includes("Firefox");const Xo=Z&&navigator.userAgent.includes("Safari")&&!Jo;function j(o){return xo(o,[255,255,255,.16])}function J(o){return xo(o,[0,0,0,.12])}const Zo=bo("n-button-group"),oe={paddingTiny:"0 6px",paddingSmall:"0 10px",paddingMedium:"0 14px",paddingLarge:"0 18px",paddingRoundTiny:"0 10px",paddingRoundSmall:"0 14px",paddingRoundMedium:"0 18px",paddingRoundLarge:"0 22px",iconMarginTiny:"6px",iconMarginSmall:"6px",iconMarginMedium:"6px",iconMarginLarge:"6px",iconSizeTiny:"14px",iconSizeSmall:"18px",iconSizeMedium:"18px",iconSizeLarge:"20px",rippleDuration:".6s"};function ee(o){const{heightTiny:x,heightSmall:b,heightMedium:d,heightLarge:n,borderRadius:G,fontSizeTiny:K,fontSizeSmall:O,fontSizeMedium:S,fontSizeLarge:W,opacityDisabled:L,textColor2:p,textColor3:oo,primaryColorHover:f,primaryColorPressed:E,borderColor:q,primaryColor:z,baseColor:i,infoColor:B,infoColorHover:H,infoColorPressed:F,successColor:r,successColorHover:l,successColorPressed:C,warningColor:e,warningColorHover:g,warningColorPressed:T,errorColor:m,errorColorHover:$,errorColorPressed:y,fontWeight:V,buttonColor2:D,buttonColor2Hover:M,buttonColor2Pressed:w,fontWeightStrong:a}=o;return Object.assign(Object.assign({},oe),{heightTiny:x,heightSmall:b,heightMedium:d,heightLarge:n,borderRadiusTiny:G,borderRadiusSmall:G,borderRadiusMedium:G,borderRadiusLarge:G,fontSizeTiny:K,fontSizeSmall:O,fontSizeMedium:S,fontSizeLarge:W,opacityDisabled:L,colorOpacitySecondary:"0.16",colorOpacitySecondaryHover:"0.22",colorOpacitySecondaryPressed:"0.28",colorSecondary:D,colorSecondaryHover:M,colorSecondaryPressed:w,colorTertiary:D,colorTertiaryHover:M,colorTertiaryPressed:w,colorQuaternary:"#0000",colorQuaternaryHover:M,colorQuaternaryPressed:w,color:"#0000",colorHover:"#0000",colorPressed:"#0000",colorFocus:"#0000",colorDisabled:"#0000",textColor:p,textColorTertiary:oo,textColorHover:f,textColorPressed:E,textColorFocus:f,textColorDisabled:p,textColorText:p,textColorTextHover:f,textColorTextPressed:E,textColorTextFocus:f,textColorTextDisabled:p,textColorGhost:p,textColorGhostHover:f,textColorGhostPressed:E,textColorGhostFocus:f,textColorGhostDisabled:p,border:`1px solid ${q}`,borderHover:`1px solid ${f}`,borderPressed:`1px solid ${E}`,borderFocus:`1px solid ${f}`,borderDisabled:`1px solid ${q}`,rippleColor:z,colorPrimary:z,colorHoverPrimary:f,colorPressedPrimary:E,colorFocusPrimary:f,colorDisabledPrimary:z,textColorPrimary:i,textColorHoverPrimary:i,textColorPressedPrimary:i,textColorFocusPrimary:i,textColorDisabledPrimary:i,textColorTextPrimary:z,textColorTextHoverPrimary:f,textColorTextPressedPrimary:E,textColorTextFocusPrimary:f,textColorTextDisabledPrimary:p,textColorGhostPrimary:z,textColorGhostHoverPrimary:f,textColorGhostPressedPrimary:E,textColorGhostFocusPrimary:f,textColorGhostDisabledPrimary:z,borderPrimary:`1px solid ${z}`,borderHoverPrimary:`1px solid ${f}`,borderPressedPrimary:`1px solid ${E}`,borderFocusPrimary:`1px solid ${f}`,borderDisabledPrimary:`1px solid ${z}`,rippleColorPrimary:z,colorInfo:B,colorHoverInfo:H,colorPressedInfo:F,colorFocusInfo:H,colorDisabledInfo:B,textColorInfo:i,textColorHoverInfo:i,textColorPressedInfo:i,textColorFocusInfo:i,textColorDisabledInfo:i,textColorTextInfo:B,textColorTextHoverInfo:H,textColorTextPressedInfo:F,textColorTextFocusInfo:H,textColorTextDisabledInfo:p,textColorGhostInfo:B,textColorGhostHoverInfo:H,textColorGhostPressedInfo:F,textColorGhostFocusInfo:H,textColorGhostDisabledInfo:B,borderInfo:`1px solid ${B}`,borderHoverInfo:`1px solid ${H}`,borderPressedInfo:`1px solid ${F}`,borderFocusInfo:`1px solid ${H}`,borderDisabledInfo:`1px solid ${B}`,rippleColorInfo:B,colorSuccess:r,colorHoverSuccess:l,colorPressedSuccess:C,colorFocusSuccess:l,colorDisabledSuccess:r,textColorSuccess:i,textColorHoverSuccess:i,textColorPressedSuccess:i,textColorFocusSuccess:i,textColorDisabledSuccess:i,textColorTextSuccess:r,textColorTextHoverSuccess:l,textColorTextPressedSuccess:C,textColorTextFocusSuccess:l,textColorTextDisabledSuccess:p,textColorGhostSuccess:r,textColorGhostHoverSuccess:l,textColorGhostPressedSuccess:C,textColorGhostFocusSuccess:l,textColorGhostDisabledSuccess:r,borderSuccess:`1px solid ${r}`,borderHoverSuccess:`1px solid ${l}`,borderPressedSuccess:`1px solid ${C}`,borderFocusSuccess:`1px solid ${l}`,borderDisabledSuccess:`1px solid ${r}`,rippleColorSuccess:r,colorWarning:e,colorHoverWarning:g,colorPressedWarning:T,colorFocusWarning:g,colorDisabledWarning:e,textColorWarning:i,textColorHoverWarning:i,textColorPressedWarning:i,textColorFocusWarning:i,textColorDisabledWarning:i,textColorTextWarning:e,textColorTextHoverWarning:g,textColorTextPressedWarning:T,textColorTextFocusWarning:g,textColorTextDisabledWarning:p,textColorGhostWarning:e,textColorGhostHoverWarning:g,textColorGhostPressedWarning:T,textColorGhostFocusWarning:g,textColorGhostDisabledWarning:e,borderWarning:`1px solid ${e}`,borderHoverWarning:`1px solid ${g}`,borderPressedWarning:`1px solid ${T}`,borderFocusWarning:`1px solid ${g}`,borderDisabledWarning:`1px solid ${e}`,rippleColorWarning:e,colorError:m,colorHoverError:$,colorPressedError:y,colorFocusError:$,colorDisabledError:m,textColorError:i,textColorHoverError:i,textColorPressedError:i,textColorFocusError:i,textColorDisabledError:i,textColorTextError:m,textColorTextHoverError:$,textColorTextPressedError:y,textColorTextFocusError:$,textColorTextDisabledError:p,textColorGhostError:m,textColorGhostHoverError:$,textColorGhostPressedError:y,textColorGhostFocusError:$,textColorGhostDisabledError:m,borderError:`1px solid ${m}`,borderHoverError:`1px solid ${$}`,borderPressedError:`1px solid ${y}`,borderFocusError:`1px solid ${$}`,borderDisabledError:`1px solid ${m}`,rippleColorError:m,waveOpacity:"0.6",fontWeight:V,fontWeightStrong:a})}const re={name:"Button",common:Do,self:ee},te=P([X("button",`
 margin: 0;
 font-weight: var(--n-font-weight);
 line-height: 1;
 font-family: inherit;
 padding: var(--n-padding);
 height: var(--n-height);
 font-size: var(--n-font-size);
 border-radius: var(--n-border-radius);
 color: var(--n-text-color);
 background-color: var(--n-color);
 width: var(--n-width);
 white-space: nowrap;
 outline: none;
 position: relative;
 z-index: auto;
 border: none;
 display: inline-flex;
 flex-wrap: nowrap;
 flex-shrink: 0;
 align-items: center;
 justify-content: center;
 user-select: none;
 -webkit-user-select: none;
 text-align: center;
 cursor: pointer;
 text-decoration: none;
 transition:
 color .3s var(--n-bezier),
 background-color .3s var(--n-bezier),
 opacity .3s var(--n-bezier),
 border-color .3s var(--n-bezier);
 `,[R("color",[v("border",{borderColor:"var(--n-border-color)"}),R("disabled",[v("border",{borderColor:"var(--n-border-color-disabled)"})]),lo("disabled",[P("&:focus",[v("state-border",{borderColor:"var(--n-border-color-focus)"})]),P("&:hover",[v("state-border",{borderColor:"var(--n-border-color-hover)"})]),P("&:active",[v("state-border",{borderColor:"var(--n-border-color-pressed)"})]),R("pressed",[v("state-border",{borderColor:"var(--n-border-color-pressed)"})])])]),R("disabled",{backgroundColor:"var(--n-color-disabled)",color:"var(--n-text-color-disabled)"},[v("border",{border:"var(--n-border-disabled)"})]),lo("disabled",[P("&:focus",{backgroundColor:"var(--n-color-focus)",color:"var(--n-text-color-focus)"},[v("state-border",{border:"var(--n-border-focus)"})]),P("&:hover",{backgroundColor:"var(--n-color-hover)",color:"var(--n-text-color-hover)"},[v("state-border",{border:"var(--n-border-hover)"})]),P("&:active",{backgroundColor:"var(--n-color-pressed)",color:"var(--n-text-color-pressed)"},[v("state-border",{border:"var(--n-border-pressed)"})]),R("pressed",{backgroundColor:"var(--n-color-pressed)",color:"var(--n-text-color-pressed)"},[v("state-border",{border:"var(--n-border-pressed)"})])]),R("loading","cursor: wait;"),X("base-wave",`
 pointer-events: none;
 top: 0;
 right: 0;
 bottom: 0;
 left: 0;
 animation-iteration-count: 1;
 animation-duration: var(--n-ripple-duration);
 animation-timing-function: var(--n-bezier-ease-out), var(--n-bezier-ease-out);
 `,[R("active",{zIndex:1,animationName:"button-wave-spread, button-wave-opacity"})]),Z&&"MozBoxSizing"in document.createElement("div").style?P("&::moz-focus-inner",{border:0}):null,v("border, state-border",`
 position: absolute;
 left: 0;
 top: 0;
 right: 0;
 bottom: 0;
 border-radius: inherit;
 transition: border-color .3s var(--n-bezier);
 pointer-events: none;
 `),v("border",`
 border: var(--n-border);
 `),v("state-border",`
 border: var(--n-border);
 border-color: #0000;
 z-index: 1;
 `),v("icon",`
 margin: var(--n-icon-margin);
 margin-left: 0;
 height: var(--n-icon-size);
 width: var(--n-icon-size);
 max-width: var(--n-icon-size);
 font-size: var(--n-icon-size);
 position: relative;
 flex-shrink: 0;
 `,[X("icon-slot",`
 height: var(--n-icon-size);
 width: var(--n-icon-size);
 position: absolute;
 left: 0;
 top: 50%;
 transform: translateY(-50%);
 display: flex;
 align-items: center;
 justify-content: center;
 `,[Ro({top:"50%",originalTransform:"translateY(-50%)"})]),Ao()]),v("content",`
 display: flex;
 align-items: center;
 flex-wrap: nowrap;
 min-width: 0;
 `,[P("~",[v("icon",{margin:"var(--n-icon-margin)",marginRight:0})])]),R("block",`
 display: flex;
 width: 100%;
 `),R("dashed",[v("border, state-border",{borderStyle:"dashed !important"})]),R("disabled",{cursor:"not-allowed",opacity:"var(--n-opacity-disabled)"})]),P("@keyframes button-wave-spread",{from:{boxShadow:"0 0 0.5px 0 var(--n-ripple-color)"},to:{boxShadow:"0 0 0.5px 4.5px var(--n-ripple-color)"}}),P("@keyframes button-wave-opacity",{from:{opacity:"var(--n-wave-opacity)"},to:{opacity:0}})]),ne=Object.assign(Object.assign({},fo.props),{color:String,textColor:String,text:Boolean,block:Boolean,loading:Boolean,disabled:Boolean,circle:Boolean,size:String,ghost:Boolean,round:Boolean,secondary:Boolean,tertiary:Boolean,quaternary:Boolean,strong:Boolean,focusable:{type:Boolean,default:!0},keyboard:{type:Boolean,default:!0},tag:{type:String,default:"button"},type:{type:String,default:"default"},dashed:Boolean,renderIcon:Function,iconPlacement:{type:String,default:"left"},attrType:{type:String,default:"button"},bordered:{type:Boolean,default:!0},onClick:[Function,Array],nativeFocusBehavior:{type:Boolean,default:!Xo},spinProps:Object}),de=po({name:"Button",props:ne,slots:Object,setup(o){const x=Q(null),b=Q(null),d=Q(!1),n=Mo(()=>!o.quaternary&&!o.tertiary&&!o.secondary&&!o.text&&(!o.color||o.ghost||o.dashed)&&o.bordered),G=ho(Zo,{}),{inlineThemeDisabled:K,mergedClsPrefixRef:O,mergedRtlRef:S,mergedComponentPropsRef:W}=_o(o),{mergedSizeRef:L}=qo({},{defaultSize:"medium",mergedSize:r=>{var l,C;const{size:e}=o;if(e)return e;const{size:g}=G;if(g)return g;const{mergedSize:T}=r||{};if(T)return T.value;const m=(C=(l=W==null?void 0:W.value)===null||l===void 0?void 0:l.Button)===null||C===void 0?void 0:C.size;return m||"medium"}}),p=N(()=>o.focusable&&!o.disabled),oo=r=>{var l;p.value||r.preventDefault(),!o.nativeFocusBehavior&&(r.preventDefault(),!o.disabled&&p.value&&((l=x.value)===null||l===void 0||l.focus({preventScroll:!0})))},f=r=>{var l;if(!o.disabled&&!o.loading){const{onClick:C}=o;C&&Ko(C,r),o.text||(l=b.value)===null||l===void 0||l.play()}},E=r=>{switch(r.key){case"Enter":if(!o.keyboard)return;d.value=!1}},q=r=>{switch(r.key){case"Enter":if(!o.keyboard||o.loading){r.preventDefault();return}d.value=!0}},z=()=>{d.value=!1},i=fo("Button","-button",te,re,o,O),B=jo("Button",S,O),H=N(()=>{const r=i.value,{common:{cubicBezierEaseInOut:l,cubicBezierEaseOut:C},self:e}=r,{rippleDuration:g,opacityDisabled:T,fontWeight:m,fontWeightStrong:$}=e,y=L.value,{dashed:V,type:D,ghost:M,text:w,color:a,round:no,circle:eo,textColor:_,secondary:go,tertiary:so,quaternary:mo,strong:yo}=o,Po={"--n-font-weight":yo?$:m};let c={"--n-color":"initial","--n-color-hover":"initial","--n-color-pressed":"initial","--n-color-focus":"initial","--n-color-disabled":"initial","--n-ripple-color":"initial","--n-text-color":"initial","--n-text-color-hover":"initial","--n-text-color-pressed":"initial","--n-text-color-focus":"initial","--n-text-color-disabled":"initial"};const A=D==="tertiary",io=D==="default",s=A?"default":D;if(w){const u=_||a;c={"--n-color":"#0000","--n-color-hover":"#0000","--n-color-pressed":"#0000","--n-color-focus":"#0000","--n-color-disabled":"#0000","--n-ripple-color":"#0000","--n-text-color":u||e[t("textColorText",s)],"--n-text-color-hover":u?j(u):e[t("textColorTextHover",s)],"--n-text-color-pressed":u?J(u):e[t("textColorTextPressed",s)],"--n-text-color-focus":u?j(u):e[t("textColorTextHover",s)],"--n-text-color-disabled":u||e[t("textColorTextDisabled",s)]}}else if(M||V){const u=_||a;c={"--n-color":"#0000","--n-color-hover":"#0000","--n-color-pressed":"#0000","--n-color-focus":"#0000","--n-color-disabled":"#0000","--n-ripple-color":a||e[t("rippleColor",s)],"--n-text-color":u||e[t("textColorGhost",s)],"--n-text-color-hover":u?j(u):e[t("textColorGhostHover",s)],"--n-text-color-pressed":u?J(u):e[t("textColorGhostPressed",s)],"--n-text-color-focus":u?j(u):e[t("textColorGhostHover",s)],"--n-text-color-disabled":u||e[t("textColorGhostDisabled",s)]}}else if(go){const u=io?e.textColor:A?e.textColorTertiary:e[t("color",s)],h=a||u,U=D!=="default"&&D!=="tertiary";c={"--n-color":U?Y(h,{alpha:Number(e.colorOpacitySecondary)}):e.colorSecondary,"--n-color-hover":U?Y(h,{alpha:Number(e.colorOpacitySecondaryHover)}):e.colorSecondaryHover,"--n-color-pressed":U?Y(h,{alpha:Number(e.colorOpacitySecondaryPressed)}):e.colorSecondaryPressed,"--n-color-focus":U?Y(h,{alpha:Number(e.colorOpacitySecondaryHover)}):e.colorSecondaryHover,"--n-color-disabled":e.colorSecondary,"--n-ripple-color":"#0000","--n-text-color":h,"--n-text-color-hover":h,"--n-text-color-pressed":h,"--n-text-color-focus":h,"--n-text-color-disabled":h}}else if(so||mo){const u=io?e.textColor:A?e.textColorTertiary:e[t("color",s)],h=a||u;so?(c["--n-color"]=e.colorTertiary,c["--n-color-hover"]=e.colorTertiaryHover,c["--n-color-pressed"]=e.colorTertiaryPressed,c["--n-color-focus"]=e.colorSecondaryHover,c["--n-color-disabled"]=e.colorTertiary):(c["--n-color"]=e.colorQuaternary,c["--n-color-hover"]=e.colorQuaternaryHover,c["--n-color-pressed"]=e.colorQuaternaryPressed,c["--n-color-focus"]=e.colorQuaternaryHover,c["--n-color-disabled"]=e.colorQuaternary),c["--n-ripple-color"]="#0000",c["--n-text-color"]=h,c["--n-text-color-hover"]=h,c["--n-text-color-pressed"]=h,c["--n-text-color-focus"]=h,c["--n-text-color-disabled"]=h}else c={"--n-color":a||e[t("color",s)],"--n-color-hover":a?j(a):e[t("colorHover",s)],"--n-color-pressed":a?J(a):e[t("colorPressed",s)],"--n-color-focus":a?j(a):e[t("colorFocus",s)],"--n-color-disabled":a||e[t("colorDisabled",s)],"--n-ripple-color":a||e[t("rippleColor",s)],"--n-text-color":_||(a?e.textColorPrimary:A?e.textColorTertiary:e[t("textColor",s)]),"--n-text-color-hover":_||(a?e.textColorHoverPrimary:e[t("textColorHover",s)]),"--n-text-color-pressed":_||(a?e.textColorPressedPrimary:e[t("textColorPressed",s)]),"--n-text-color-focus":_||(a?e.textColorFocusPrimary:e[t("textColorFocus",s)]),"--n-text-color-disabled":_||(a?e.textColorDisabledPrimary:e[t("textColorDisabled",s)])};let ro={"--n-border":"initial","--n-border-hover":"initial","--n-border-pressed":"initial","--n-border-focus":"initial","--n-border-disabled":"initial"};w?ro={"--n-border":"none","--n-border-hover":"none","--n-border-pressed":"none","--n-border-focus":"none","--n-border-disabled":"none"}:ro={"--n-border":e[t("border",s)],"--n-border-hover":e[t("borderHover",s)],"--n-border-pressed":e[t("borderPressed",s)],"--n-border-focus":e[t("borderFocus",s)],"--n-border-disabled":e[t("borderDisabled",s)]};const{[t("height",y)]:to,[t("fontSize",y)]:So,[t("padding",y)]:To,[t("paddingRound",y)]:$o,[t("iconSize",y)]:wo,[t("borderRadius",y)]:zo,[t("iconMargin",y)]:Ho,waveOpacity:Fo}=e,Io={"--n-width":eo&&!w?to:"initial","--n-height":w?"initial":to,"--n-font-size":So,"--n-padding":eo||w?"initial":no?$o:To,"--n-icon-size":wo,"--n-icon-margin":Ho,"--n-border-radius":w?"initial":eo||no?to:zo};return Object.assign(Object.assign(Object.assign(Object.assign({"--n-bezier":l,"--n-bezier-ease-out":C,"--n-ripple-duration":g,"--n-opacity-disabled":T,"--n-wave-opacity":Fo},Po),c),ro),Io)}),F=K?No("button",N(()=>{let r="";const{dashed:l,type:C,ghost:e,text:g,color:T,round:m,circle:$,textColor:y,secondary:V,tertiary:D,quaternary:M,strong:w}=o;l&&(r+="a"),e&&(r+="b"),g&&(r+="c"),m&&(r+="d"),$&&(r+="e"),V&&(r+="f"),D&&(r+="g"),M&&(r+="h"),w&&(r+="i"),T&&(r+=`j${co(T)}`),y&&(r+=`k${co(y)}`);const{value:a}=L;return r+=`l${a[0]}`,r+=`m${C[0]}`,r}),H,o):void 0;return{selfElRef:x,waveElRef:b,mergedClsPrefix:O,mergedFocusable:p,mergedSize:L,showBorder:n,enterPressed:d,rtlEnabled:B,handleMousedown:oo,handleKeydown:q,handleBlur:z,handleKeyup:E,handleClick:f,customColorCssVars:N(()=>{const{color:r}=o;if(!r)return null;const l=j(r);return{"--n-border-color":r,"--n-border-color-hover":l,"--n-border-color-pressed":J(r),"--n-border-color-focus":l,"--n-border-color-disabled":r}}),cssVars:K?void 0:H,themeClass:F==null?void 0:F.themeClass,onRender:F==null?void 0:F.onRender}},render(){const{mergedClsPrefix:o,tag:x,onRender:b}=this;b==null||b();const d=ao(this.$slots.default,n=>n&&I("span",{class:`${o}-button__content`},n));return I(x,{ref:"selfElRef",class:[this.themeClass,`${o}-button`,`${o}-button--${this.type}-type`,`${o}-button--${this.mergedSize}-type`,this.rtlEnabled&&`${o}-button--rtl`,this.disabled&&`${o}-button--disabled`,this.block&&`${o}-button--block`,this.enterPressed&&`${o}-button--pressed`,!this.text&&this.dashed&&`${o}-button--dashed`,this.color&&`${o}-button--color`,this.secondary&&`${o}-button--secondary`,this.loading&&`${o}-button--loading`,this.ghost&&`${o}-button--ghost`],tabindex:this.mergedFocusable?0:-1,type:this.attrType,style:this.cssVars,disabled:this.disabled,onClick:this.handleClick,onBlur:this.handleBlur,onMousedown:this.handleMousedown,onKeyup:this.handleKeyup,onKeydown:this.handleKeydown},this.iconPlacement==="right"&&d,I(Go,{width:!0},{default:()=>ao(this.$slots.icon,n=>(this.loading||this.renderIcon||n)&&I("span",{class:`${o}-button__icon`,style:{margin:Wo(this.$slots.default)?"0":""}},I(ko,null,{default:()=>this.loading?I(Oo,Object.assign({clsPrefix:o,key:"loading",class:`${o}-icon-slot`,strokeWidth:20},this.spinProps)):I("div",{key:"icon",class:`${o}-icon-slot`,role:"none"},this.renderIcon?this.renderIcon():n)})))}),this.iconPlacement==="left"&&d,this.text?null:I(Yo,{ref:"waveElRef",clsPrefix:o}),this.showBorder?I("div",{"aria-hidden":!0,class:`${o}-button__border`,style:this.customColorCssVars}):null,this.showBorder?I("div",{"aria-hidden":!0,class:`${o}-button__state-border`,style:this.customColorCssVars}):null)}}),ce=(o,x)=>{const b=o.__vccOpts||o;for(const[d,n]of x)b[d]=n;return b};export{de as B,ce as _,Xo as a,re as b,co as c,ae as e,uo as f,Z as i,le as m,qo as u};
