// 导入核心脚本、国际化支持、事件系统和弹出窗口模块
import { chat, isChatSaving, this_edit_mes_id } from '../script.js';
import { t } from './i18n.js';
import { eventSource, event_types } from './events.js';
import { callGenericPopup, POPUP_TYPE } from './popup.js';

// 全局无障碍状态控制变量
let isA11yEnabled = true; // 标记无障碍功能是否启用
let mainObserver = null;  // 用于监听DOM变化的MutationObserver实例

// 调试焦点相关日志配置
const DEBUG_FOCUS = false;

/**
 * 打印调试日志，专门用于跟踪无障碍相关操作
 * @param {string} location - 日志发生的具体位置/函数名
 * @param {string} message - 日志信息
 * @param {any} data - 可选的附加数据
 */
function logDebug(location, message, data = null) {
  if (!DEBUG_FOCUS) return;
  const time = new Date().toISOString().split('T')[1].slice(0, -1);
  const css = 'color: #00bcd4; font-weight: bold;';
  if (data) {
    console.log(`%c[A11y][${time}][${location}] ${message}`, css, data);
  } else {
    console.log(`%c[A11y][${time}][${location}] ${message}`, css);
  }
}

// ----------------------------------------------------------------------------
// CSS选择器定义区：用于批量选中需要注入无障碍属性的DOM元素
// ----------------------------------------------------------------------------

// 需要转换为按钮角色 (role="button") 的元素选择器
const buttonSelectors = [
  '.menu_button',
  '.right_menu_button',
  '.killSwitch',
  '.mes_button',
  '.drawer-icon',
  '.drawer-opener',
  '.swipe_left',
  '.swipe_right',
  '.character_select',
  '.tags .tag',
  '.jg-menu .jg-button',
  '.bg_example .mobile-only-menu-toggle',
  '.paginationjs-pages li a',
  '.inline-drawer-toggle',
  '.qr--action',
  '.a11y-sort-button',
  '.extensions_toolbar button',
].join(', ');

// 需要转换为列表角色 (role="list") 的元素选择器
const listSelectors = [
  '.options-content',
  '.list-group',
  '.list-group-item',
  '#rm_print_characters_block',
  '#rm_group_members',
  '#rm_group_add_members',
  '.tag_view_list_tags',
  '.secretKeyManagerList',
  '.recentChatList',
  '.dataMaidCategoryContent',
  '#userList',
  '.bg_list',
  '.qr--setList',
  '.qr--set-qrListContents',
  '#completion_prompt_manager_list',
  '.regex-debugger-rules-list ul',
  '.regex-script-container',
].join(', ');

// 需要转换为列表项角色 (role="listitem") 的元素选择器
const listItemSelectors = [
  '.options-content .list-group-item',
  '.list-group .list-group-item',
  '#rm_print_characters_block .entity_block',
  '#rm_group_members .group_member',
  '#rm_group_add_members .group_member',
  '.tag_view_list_tags .tag_view_item',
  '.secretKeyManagerList .secretKeyManagerItem',
  '.recentChatList .recentChat',
  '.dataMaidCategoryContent .dataMaidItem',
  '#userList .userSelect',
  '.bg_list .bg_example',
  '.qr--item',
  '.qr--set-item',
  '.completion_prompt_manager_prompt',
  '.regex-debugger-rule',
  '.regex-script-label',
  '.extension_block',
  '.extension_container',
].join(', ');

// 需要转换为工具栏角色 (role="toolbar") 的元素选择器
const toolbarSelectors = [
  '.jg-menu',
  '.qr--head',
  '.regex_bulk_operations',
  '.extensions_toolbar',
].join(', ');

// 选项卡列表选择器 (role="tablist")
const tabListSelectors = ['#bg_tabs .bg_tabs_list'].join(', ');
// 选项卡项选择器 (role="tab")
const tabItemSelectors = ['#bg_tabs .bg_tabs_list .bg_tab_button'].join(', ');

// 用于标记元素已经被注入了通用无障碍属性的自定义属性
const GENERIC_ATTR = 'data-a11y-generic';

/**
 * 核心无障碍处理器集合
 * 定义了为各类元素添加正确 ARIA 角色和 tabindex（使其可获得键盘焦点）的方法
 */
export const a11yProcessors = {
  button: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'button');
    // 如果不是原生按钮或链接，需要赋予 tabindex="0" 以支持键盘Tab切换
    if (
      !element.hasAttribute('tabindex') &&
      element.tagName !== 'BUTTON' &&
      element.tagName !== 'A'
    ) {
      element.setAttribute('tabindex', '0');
    }
  },
  list: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'list');
  },
  listItem: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'listitem');
    // 特定类型的列表项如果有 tabindex="0"，则移除它，通常是因为它们包含自己的可聚焦子元素
    if (
      element.hasAttribute('tabindex') &&
      element.getAttribute('tabindex') === '0' &&
      (element.classList.contains('completion_prompt_manager_prompt') ||
        element.classList.contains('qr--item') ||
        element.classList.contains('regex-script-label') ||
        element.classList.contains('list-group-item'))
    ) {
      element.removeAttribute('tabindex');
    }
  },
  toolbar: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'toolbar');
  },
  tabList: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'tablist');
  },
  tab: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'tab');
  },
  status: (element) => {
    if (!element.hasAttribute('role')) element.setAttribute('role', 'status');
  },
};

// 创建一个注册表，将选择器映射到对应的处理器
const a11yRegistry = new Map();
a11yRegistry.set(buttonSelectors, a11yProcessors.button);
a11yRegistry.set(listSelectors, a11yProcessors.list);
a11yRegistry.set(listItemSelectors, a11yProcessors.listItem);
a11yRegistry.set(toolbarSelectors, a11yProcessors.toolbar);
a11yRegistry.set(tabListSelectors, a11yProcessors.tabList);
a11yRegistry.set(tabItemSelectors, a11yProcessors.tab);
a11yRegistry.set('#toast-container .toast', a11yProcessors.status); // 为系统通知添加状态角色

/**
 * 动态注册新的无障碍选择器及其处理器（可被外部模块调用）
 */
export function registerA11ySelector(selector, processor) {
  let finalProcessor;
  if (typeof processor === 'string') {
    if (a11yProcessors[processor]) {
      finalProcessor = a11yProcessors[processor];
    } else {
      console.warn(`[A11y] Unknown processor type: ${processor}. Defaulting to no-op.`);
      return;
    }
  } else if (typeof processor === 'function') {
    finalProcessor = processor;
  } else {
    console.warn('[A11y] Processor must be a function or a valid processor key.');
    return;
  }

  a11yRegistry.set(selector, finalProcessor);

  // 如果A11y功能已开启，立即对页面中现有的匹配元素进行处理
  if (isA11yEnabled) {
    try {
      document.querySelectorAll(selector).forEach((el) => {
        if (!el.hasAttribute(GENERIC_ATTR)) {
          finalProcessor(el);
          el.setAttribute(GENERIC_ATTR, 'true');
        }
      });
    } catch (e) {
      console.warn(`[A11y] Failed to apply new rule for selector "${selector}":`, e);
    }
  }
}

/**
 * 为指定的 DOM 节点及其子节点应用所有通用的无障碍规则
 * @param {Element} rootElement - 根元素（通常是新插入 DOM 的节点或 document.body）
 */
function applyGenericA11yRules(rootElement) {
  try {
    // 检查根元素自身是否匹配
    if (rootElement.nodeType === 1 && !rootElement.hasAttribute(GENERIC_ATTR)) {
      for (const [selector, rule] of a11yRegistry.entries()) {
        if (rootElement.matches(selector)) {
          rule(rootElement);
          rootElement.setAttribute(GENERIC_ATTR, 'true');
        }
      }
    }
    // 遍历所有子元素
    for (const [selector, rule] of a11yRegistry.entries()) {
      const elements = rootElement.querySelectorAll(selector);
      for (let i = 0; i < elements.length; i++) {
        const el = elements[i];
        if (!el.hasAttribute(GENERIC_ATTR)) {
          rule(el);
          el.setAttribute(GENERIC_ATTR, 'true');
        }
      }
    }
  } catch (error) {
    console.error('Error applying accessibility rules:', error);
  }
}

/**
 * 处理列表项（如提示词、快速回复、正则脚本）的无障碍排序菜单
 * 允许用户通过键盘弹出对话框进行上移、下移、置顶、置底和跳转操作
 */
async function handleSortMenu(triggerElement, itemSelector, containerSelector) {
  const $trigger = $(triggerElement);
  const $li = $trigger.closest(itemSelector);
  const $container = $li.closest(containerSelector);
  const $allItems = $container.children(itemSelector);
  const total = $allItems.length;
  const currentIndex = $allItems.index($li);
  const displayIndex = currentIndex + 1;

  // 尝试获取当前项的名称，以便在弹窗中显示
  let itemName =
    $li
      .find(
        '.completion_prompt_manager_prompt_name, .qr--set option:selected, .qr--set-itemLabel, .regex_script_name',
      )
      .first()
      .val() ||
    $li
      .find(
        '.completion_prompt_manager_prompt_name, .qr--set option:selected, .qr--set-itemLabel, .regex_script_name',
      )
      .first()
      .text() ||
    'Item';
  itemName = String(itemName).trim();

  // 唤起通用弹窗提供排序选项
  const popupPromise = callGenericPopup(
    `<h3>${t`Sort Item`}</h3><p>${t`Move`} <b>${itemName}</b> (${t`Position ${displayIndex} of ${total}`})</p>`,
    POPUP_TYPE.TEXT,
    '',
    {
      okButton: t`Close`,
      cancelButton: false,
      wide: true,
      customButtons: [
        {
          text: t`Move Up`,
          action: () => performGenericSortAction($li, $container, itemSelector, 'up'),
        },
        {
          text: t`Move Down`,
          action: () => performGenericSortAction($li, $container, itemSelector, 'down'),
        },
        {
          text: t`To Top`,
          action: () => performGenericSortAction($li, $container, itemSelector, 'top'),
        },
        {
          text: t`To Bottom`,
          action: () => performGenericSortAction($li, $container, itemSelector, 'bottom'),
        },
        {
          text: t`Jump to...`,
          action: () =>
            setTimeout(() => handleGenericJumpAction($li, $container, itemSelector), 150),
        },
      ],
    },
  );

  // 处理弹窗内的焦点控制（支持按Esc关闭弹窗）
  setTimeout(() => {
    const $popup = $('.popup:visible').last();
    if ($popup.length) {
      $popup.attr('tabindex', '-1').focus();
      $popup.on('keydown.a11ySort', (e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          e.stopPropagation();
          $popup.find('.popup-button-ok').trigger('click');
        }
      });
    }
  }, 50);

  try {
    await popupPromise;
  } finally {
    // 弹窗关闭后，清除事件监听并将焦点交还给原始按钮或新位置的排序按钮
    $('.popup').off('keydown.a11ySort');
    setTimeout(() => {
      if ($trigger.closest('body').length) {
        $trigger.trigger('focus');
      } else {
        const $newLi = $container.children(itemSelector).eq(currentIndex);
        $newLi.find('.a11y-sort-button').trigger('focus');
      }
    }, 150);
  }
}

/**
 * 排序操作：处理跳转到特定编号的逻辑
 */
async function handleGenericJumpAction($item, $container, itemSelector) {
  const max = $container.children(itemSelector).length;
  logDebug('JumpAction', `Requesting input 1-${max}`);
  const input = await callGenericPopup(t`Enter new position (1-${max}):`, POPUP_TYPE.INPUT, '', {
    okButton: t`Move`,
  });
  if (input) {
    const targetPos = parseInt(String(input));
    if (!isNaN(targetPos) && targetPos >= 1 && targetPos <= max) {
      logDebug('JumpAction', `Jumping to ${targetPos}`);
      performGenericSortAction($item, $container, itemSelector, 'jump', targetPos - 1);
    } else {
      announceA11y(t`Invalid position number.`);
      $item.find('.a11y-sort-button').trigger('focus');
    }
  } else {
    logDebug('JumpAction', 'Cancelled. Returning focus.');
    $item.find('.a11y-sort-button').trigger('focus');
  }
}

/**
 * 执行通用排序逻辑的底层方法，并向屏幕阅读器播报结果
 */
function performGenericSortAction($item, $container, itemSelector, action, targetIndex = null) {
  const validSelectors =
    '.qr--item, .qr--set-item, .completion_prompt_manager_prompt, .regex-script-label, .list-group-item';
  let $allItems = $container.children(validSelectors);
  const total = $allItems.length;
  const currentIndex = $allItems.index($item);
  let changed = false;
  let actionText = '';
  let targetName = '';

  // 辅助函数：提取元素的无障碍名称
  const getA11yName = ($el) =>
    (
      $el
        .find(
          '.completion_prompt_manager_prompt_name, .qr--set option:selected, .qr--set-itemLabel, .regex_script_name',
        )
        .first()
        .val() ||
      $el
        .find(
          '.completion_prompt_manager_prompt_name, .qr--set option:selected, .qr--set-itemLabel, .regex_script_name',
        )
        .first()
        .text() ||
      'Item'
    ).trim();

  // 判断当前请求的操作类型并修改DOM
  if (action === 'up' && currentIndex > 0) {
    const $other = $allItems.eq(currentIndex - 1);
    targetName = getA11yName($other);
    $item.insertBefore($other);
    changed = true;
    actionText = t`Swapped with ${targetName}`;
  } else if (action === 'down' && currentIndex < total - 1) {
    const $other = $allItems.eq(currentIndex + 1);
    targetName = getA11yName($other);
    $item.insertAfter($other);
    changed = true;
    actionText = t`Swapped with ${targetName}`;
  } else if (action === 'top' && currentIndex > 0) {
    $item.prependTo($container);
    changed = true;
    actionText = t`Moved to top`;
  } else if (action === 'bottom' && currentIndex < total - 1) {
    $item.appendTo($container);
    changed = true;
    actionText = t`Moved to bottom`;
  } else if (action === 'jump' && targetIndex !== null) {
    if (targetIndex >= 0 && targetIndex < total && targetIndex !== currentIndex) {
      const $target = $allItems.eq(targetIndex);
      targetName = getA11yName($target);
      if (currentIndex < targetIndex) $item.insertAfter($target);
      else $item.insertBefore($target);
      changed = true;
      actionText = t`Moved to position ${targetIndex + 1}`;
    }
  }

  if (changed) {
    // 触发外部框架（如 jQuery UI Sortable）更新事件
    if ($container.data('ui-sortable')) {
      $container.sortable('refresh');
    }
    $container.trigger('sortupdate');

    // 计算移动后的新位置
    const $newAllItems = $container.children(validSelectors);
    const newIndex = $newAllItems.index($item) + 1;
    const finalMessage = t`${actionText}. Position ${newIndex} of ${total}.`;

    // 更新焦点并通知屏幕阅读器
    const $popup = $('.popup:visible');
    let btnType = '';
    if (action === 'up') btnType = t`Move Up`;
    else if (action === 'down') btnType = t`Move Down`;
    else if (action === 'top') btnType = t`To Top`;
    else if (action === 'bottom') btnType = t`To Bottom`;

    if ($popup.length && btnType) {
      const $btn = $popup.find('.popup-button-custom').filter(function () {
        return $(this).text().trim() === btnType;
      });
      if ($btn.length) {
        $btn.attr('aria-label', finalMessage);
        $btn.trigger('focus');
        setTimeout(() => {
          $btn.removeAttr('aria-label');
        }, 2e3);
        return;
      }
    }
    announceA11y(finalMessage, true);
  } else {
    announceA11y(t`Already at limit.`, true); // 提示已经到顶或到底了
  }
}

// 记录 AI 是否正在生成的标志位
let isAiGenerating = false;

/**
 * 向屏幕阅读器播报信息 (利用 ARIA Live Region)
 * @param {string} text - 需要播报的文本内容
 * @param {boolean} force - 如果为 true，使用 assertive 模式打断当前语音，否则用 polite 模式
 */
export function announceA11y(text, force = false) {
  if (!text) return;
  console.log(`%c[A11y] ${text}`, 'color: #4caf50');
  let announcer = document.getElementById('a11y-announcer');

  // 动态创建一个不可见的 DOM 节点用于播报
  if (!announcer) {
    announcer = document.createElement('div');
    announcer.id = 'a11y-announcer';
    Object.assign(announcer.style, {
      position: 'absolute',
      width: '1px',
      height: '1px',
      padding: '0',
      margin: '-1px',
      overflow: 'hidden',
      clip: 'rect(0, 0, 0, 0)',
      whiteSpace: 'nowrap',
      border: '0',
    });
    document.body.appendChild(announcer);
  }
  announcer.textContent = '';

  if (force) {
    announcer.setAttribute('role', 'alert');
    announcer.setAttribute('aria-live', 'assertive');
  } else {
    announcer.setAttribute('role', 'status');
    announcer.setAttribute('aria-live', 'polite');
  }

  // 使用延时确保屏幕阅读器捕捉到文本的变化
  setTimeout(() => {
    announcer.textContent = text;
  }, 50);
}

/**
 * 抽屉式折叠面板的焦点处理逻辑
 */
export function handleDrawerFocus(triggerButton, drawerElement, isOpening) {
  if (!isA11yEnabled) return;
  if (isOpening) {
    triggerButton.attr('aria-expanded', 'true');
  } else {
    triggerButton.attr('aria-expanded', 'false');
    triggerButton.trigger('focus');
  }
}

// ----------------------------------------------------------------------------
// 特定模块处理器集合：针对界面上复杂的组合UI提供自定义的无障碍修复逻辑
// ----------------------------------------------------------------------------
const SpecificProcessors = {
  // 静态页面固定修复（处理特定 ID 和结构不规范的标签）
  staticFixes: (root) => {
    const findId = (id) => {
      const el = document.getElementById(id);
      return el && root.contains(el) ? $(el) : null;
    };

    // 修复 Assets JSON URL 输入框的关联标签
    const $assetsField = findId('assets-json-url-field');
    if ($assetsField && !$assetsField.attr('aria-labelledby')) {
      const $mainLabel = $('label[for="assets-json-url-field"]');
      const $hintSpan = $assetsField
        .closest('.assets-url-block')
        .find('small span[data-i18n="Load an asset list"]');
      if ($mainLabel.length && $hintSpan.length) {
        const labelId = $mainLabel.attr('id') || 'label-assets-url';
        $mainLabel.attr('id', labelId);
        const hintId = $hintSpan.attr('id') || 'label-assets-hint';
        $hintSpan.attr('id', hintId);
        $assetsField.attr('aria-labelledby', `${labelId} ${hintId}`);
      }
    }

    // 扩展页面标题与提示框关联
    const $extTitle = findId('rm_extensions_block');
    if ($extTitle) {
      $extTitle.find('h3[data-i18n="Extensions"]').attr('id', 'title_extensions');
      findId('extensions_notify_updates')?.attr(
        'aria-labelledby',
        'label-extensions_notify_updates',
      );
    }

    // 表情API设置的标签绑定
    ['expression_api', 'expression_fallback'].forEach((id) => {
      const $el = findId(id);
      if ($el && !$el.attr('aria-labelledby')) {
        const $label = $(`label[for="${id}"]`);
        if ($label.length) {
          const labelId = $label.attr('id') || `a11y-label-${id}`;
          $label.attr('id', labelId);
          $el.attr('aria-labelledby', labelId);
        }
      }
    });

    // 图像生成配置项的无障碍描述（提取 title 属性作为 aria-label）
    const imgGenFixes = [
      'sd_refine_mode',
      'sd_function_tool',
      'sd_interactive_mode',
      'sd_multimodal_captioning',
      'sd_free_extend',
      'sd_snap',
      'sd_minimal_prompt_processing',
      'sd_novel_anlas_guard',
    ];
    imgGenFixes.forEach((id) => {
      const $el = findId(id);
      if ($el && !$el.attr('aria-label')) {
        const title = $el.parent().attr('title');
        if (title) $el.attr('aria-label', title);
      }
    });

    // 修复聊天可见性相关的多选框描述
    const $visHeader = $('h4[data-i18n="Chat Message Visibility (by source)"]');
    if ($visHeader.length && root.contains($visHeader[0])) {
      const $visDesc = $visHeader.next('small');
      if ($visDesc.length) {
        $visDesc.attr('id', 'sd-vis-desc');
        $('#sd_wand_visible, #sd_command_visible, #sd_interactive_visible, #sd_tool_visible').each(
          function () {
            if (!$(this).attr('aria-describedby')) $(this).attr('aria-describedby', 'sd-vis-desc');
          },
        );
      }
    }

    // Stable Diffusion 提示词模板的文本框关联
    const $sdPrompt = findId('sd_prompt_templates');
    if ($sdPrompt) {
      $sdPrompt.find('textarea').each(function () {
        if ($(this).attr('aria-labelledby')) return;
        const id = this.id;
        const $labelWrapper = $(this).prev('.title_restorable');
        if ($labelWrapper.length) {
          const $label = $labelWrapper.find(`label[for="${id}"]`);
          if ($label.length) {
            const labelId = $label.attr('id') || `a11y-lbl-${id}`;
            $label.attr('id', labelId);
            $(this).attr('aria-labelledby', labelId);
            $labelWrapper
              .find('.menu_button.fa-undo')
              .attr({
                role: 'button',
                tabindex: '0',
                'aria-label': 'Restore default: ' + $label.text(),
              });
          }
        }
      });
    }

    // TTS (文本转语音) 提供商下拉框的标签修复
    const $ttsProvider = findId('tts_provider');
    if ($ttsProvider && !$ttsProvider.attr('aria-labelledby')) {
      const $lbl = $('#drawer-n8gxyt > span[data-i18n="Select TTS Provider"]');
      if ($lbl.length) {
        $lbl.attr('id', 'lbl-tts-provider-text');
        $ttsProvider
          .attr('aria-labelledby', 'lbl-tts-provider-text')
          .removeAttr('aria-describedby');
      }
    }
    findId('tts_refresh')?.attr('aria-label', 'Reload TTS Provider');

    // Caption 来源设置修复
    const $capSrc = findId('caption_source');
    if ($capSrc && !$capSrc.attr('aria-labelledby')) {
      const $lbl = $('label[for="caption_source"]');
      if ($lbl.length) {
        const id = $lbl.attr('id') || 'lbl-caption-source';
        $lbl.attr('id', id);
        $capSrc.attr('aria-labelledby', id).removeAttr('aria-describedby');
      }
    }

    // API 设置模块下的表单项复杂关联逻辑（往前回溯寻找标题节点当做 Label）
    if ($(root).find('#rm_api_block').length || $(root).is('#rm_api_block')) {
      $('#rm_api_block select, #rm_api_block input').each(function () {
        const $el = $(this);
        if ($el.attr('id') === 'main_api') return;
        let $label = null;
        if ($el.attr('id')) {
          const $forLabel = $(`label[for="${$el.attr('id')}"]`);
          if ($forLabel.length) $label = $forLabel;
        }
        if (!$label || !$label.length) {
          let $current = $el.parent();
          if (
            $current.hasClass('flex-container') ||
            $current.hasClass('wide100p') ||
            $current.hasClass('openai_logit_bias_preset_form') ||
            $current.is('div')
          ) {
            let $prev = $current.prev();
            while ($prev.length) {
              if ($prev.is('.range-block-title, h3, h4, h5, label, strong, b')) {
                $label = $prev;
                break;
              }
              // 跳过说明文字和警告信息，继续往前找
              if (
                $prev.is(
                  '.toggle-description, .neutral_warning, small, hr, .inline-drawer-toggle',
                ) ||
                $prev.hasClass('notes-link')
              ) {
                $prev = $prev.prev();
              } else {
                break;
              }
            }
          }
        }
        if ($label && $label.length && !$label.closest('.neutral_warning').length) {
          let labelId =
            $label.attr('id') ||
            'lbl-' + ($el.attr('id') || Math.random().toString(36).substr(2, 5));
          $label.attr('id', labelId);
          $el.attr('aria-labelledby', labelId);
        }
      });
    }
  },

  // 聊天界面的无障碍优化
  chat: (root) => {
    const $root = $(root);
    const $messages = $root.find('#chat .mes').addBack('#chat .mes');
    $messages.each(function () {
      const $mes = $(this);
      if ($mes.hasClass('a11y-refactored')) return; // 防止重复处理

      const $nameText = $mes.find('.name_text');
      const charName = $nameText.text() || 'System';
      const isUser = $mes.attr('is_user') === 'true';
      const timestamp = $mes.find('.timestamp').text().trim();
      const isLast = $mes.is(':last-child'); // 只有最新的一条消息可以进入 Tab 序列

      // 生成消息发送者名字的ID
      let headingId = $nameText.attr('id');
      if (!headingId && $nameText.length) {
        headingId = 'mes-heading-' + Math.random().toString(36).substr(2, 5);
        $nameText.attr('id', headingId);
      }

      // 生成消息正文的ID
      const $mesText = $mes.find('.mes_text');
      let textId = $mesText.attr('id');
      if (!textId && $mesText.length) {
        textId = 'mes-text-' + Math.random().toString(36).substr(2, 5);
        $mesText.attr('id', textId);
      }

      // 将发送者名称和消息正文作为整个消息框的 aria-labelledby
      let labelledby = [];
      if (headingId) labelledby.push(headingId);
      if (textId) labelledby.push(textId);

      $mes
        .attr({
          role: 'article', // 聊天消息作为独立的 article
          tabindex: isLast ? '0' : '-1',
          'aria-labelledby': labelledby.length > 0 ? labelledby.join(' ') : undefined,
        })
        .addClass('a11y-refactored');

      if ($nameText.length) {
        const youStr = t`You`;
        $nameText.attr({
          role: 'heading',
          'aria-level': '3', // 名字作为3级标题
          'aria-label': `${isUser ? youStr : charName} ${timestamp ? ', ' + timestamp : ''}`,
        });
      }

      // 屏蔽不必要的屏幕阅读器冗余信息，添加必要的操作标签
      $mes.find('.swipe_left').attr('aria-label', t`Swipe Left`);
      $mes.find('.swipe_right').attr('aria-label', t`Swipe Right`);
      $mes
        .find('.mesIDDisplay, .drag-handle, .swipes-counter, .mes_timer, .timestamp')
        .attr('aria-hidden', 'true');
    });
  },

  // 智能寻找表单输入控件（input、textarea、select）并自动绑定周围的标签（Label）
  inputs: (root) => {
    const $root = $(root);
    const $inputs = $root.find('input, textarea, select').addBack('input, textarea, select');
    $inputs.each(function () {
      const $el = $(this);
      if ($el.is('[type="hidden"]')) return;
      if ($el.attr('aria-labelledby') && document.getElementById($el.attr('aria-labelledby')))
        return;

      // 确保有ID
      let id = $el.attr('id');
      if (!id) {
        id = 'st-a11y-' + Math.random().toString(36).substr(2, 5);
        $el.attr('id', id);
      }

      let $label = null;
      if ($el.attr('id')) {
        const $forLabel = $(`label[for="${$el.attr('id')}"]`);
        if ($forLabel.length) $label = $forLabel;
      }

      // 如果没有显式的 for="..."，则在 DOM 树前方进行智能查找
      if (!$label || !$label.length) {
        let $curr = $el;
        for (let i = 0; i < 4; i++) {
          let $prev = $curr.prev();
          let attempts = 0;
          while ($prev.length && attempts < 5) {
            // 匹配常见的标题或说明元素作为标签
            if ($prev.is('.range-block-title, h4, h3, h5, label, strong, b')) {
              $label = $prev;
              break;
            }
            const $nestedTitle = $prev
              .find('.range-block-title, h4, h3, h5, label, strong, b')
              .first();
            if ($nestedTitle.length) {
              $label = $nestedTitle;
              break;
            }
            if (
              $prev.is(
                '.toggle-description, .neutral_warning, small, hr, .inline-drawer-toggle, .notes-link, .fa-circle-info',
              ) ||
              $prev.hasClass('notes-link') ||
              $prev.text().trim() === ''
            ) {
              $prev = $prev.prev();
              attempts++;
            } else {
              break;
            }
          }
          if ($label && $label.length) break;
          const $parent = $curr.parent();
          if (
            $parent.length &&
            ($parent.hasClass('range-block-range') ||
              $parent.hasClass('range-block-range-and-counter') ||
              $parent.hasClass('range-block') ||
              $parent.hasClass('wide100p') ||
              $parent.hasClass('flex-container') ||
              $parent.hasClass('oneline-dropdown') ||
              $parent.is('div'))
          ) {
            $curr = $parent;
          } else {
            break;
          }
        }
      }

      // 备用：从父容器里找标题
      if (!$label || !$label.length) {
        const $container = $el.closest('.range-block');
        if ($container.length) {
          $label = $container.find('.range-block-title, h4, h3, label').first();
        }
      }

      // 找到标签后，将二者使用 aria-labelledby 绑定起来
      if ($label && $label.length) {
        if ($label.is($el)) return;
        if ($label.closest('.neutral_warning').length) return;
        if (
          $label.children().length > 0 &&
          !$label.text().trim() &&
          $label.find('span, b, strong').length
        ) {
          $label = $label.find('span, b, strong').first();
        }
        const titleId = $label.attr('id') || 'label-' + id;
        $label.attr('id', titleId);
        $el.attr('aria-labelledby', titleId);
      }

      // 绑定说明性文本（aria-describedby）
      const $descContainer = $el.closest('.range-block, .wide100p, .flex-container');
      if ($descContainer.length) {
        const $desc = $descContainer
          .find('.text_muted, .toggle-description, small.flexBasis100p')
          .filter(function () {
            return $(this).text().trim().length > 0;
          })
          .first();
        if ($desc.length && !$el.attr('aria-describedby')) {
          const descId = $desc.attr('id') || 'desc-' + id;
          $desc.attr('id', descId);
          $el.attr('aria-describedby', descId);
        }
      }

      // 对数字输入框做额外处理
      if ($el.is('[type="number"]')) {
        $el.attr('role', 'spinbutton');
        const min = $el.attr('min'),
          max = $el.attr('max');
        if (min !== undefined) $el.attr('aria-valuemin', min);
        if (max !== undefined) $el.attr('aria-valuemax', max);
      }
    });

    // Select2 插件的多选删除按钮处理
    $root
      .find('.select2-selection__choice__remove')
      .addBack('.select2-selection__choice__remove')
      .each(function () {
        const $btn = $(this);
        if ($btn.attr('tabindex')) return;
        $btn.attr('tabindex', '0');
        const $item = $btn.closest('.select2-selection__choice');
        const title =
          $item.attr('title') || $item.find('.select2-selection__choice__display').text();
        if (title) $btn.attr('aria-label', `Remove ${title}`);
      });
  },

  // 修复折叠面板 (inline-drawers) 的aria-expanded展开状态和aria-controls关联
  drawers: (root) => {
    const $root = $(root);
    $root
      .find('.inline-drawer')
      .addBack('.inline-drawer')
      .each(function () {
        const $drawer = $(this);
        const $header = $drawer.children('.inline-drawer-toggle');
        if ($header.attr('aria-controls')) return;

        const $content = $drawer.children('.inline-drawer-content');
        const $icon = $header.find('.inline-drawer-icon');
        if (!$content.length || !$header.length) return;

        let contentId = $content.attr('id');
        if (!contentId) {
          contentId = 'drawer-' + Math.random().toString(36).substr(2, 6);
          $content.attr('id', contentId);
        }

        const isExpanded = $content.is(':visible');
        $header.attr({
          role: 'button',
          tabindex: '0',
          'aria-expanded': isExpanded ? 'true' : 'false',
          'aria-controls': contentId,
        });

        // 隐藏图标以防止屏幕阅读器读取冗余内容
        $icon.attr({ 'aria-hidden': 'true', tabindex: '-1' }).removeAttr('role');
        const $title = $header.find('b, strong, span').first();
        if ($title.length) {
          const titleId = $title.attr('id') || 'title-' + contentId;
          $title.attr('id', titleId);
          $header.attr('aria-labelledby', titleId);
        }
      });
  },

  // 左右导航面板和角色管理面板的按钮标签处理
  navAndCharPanel: (root) => {
    const $root = $(root);

    // 面板锁定/解锁复选框的标签修复
    $root
      .find('#lm_button_panel_pin_div, #rm_button_panel_pin_div')
      .addBack('#lm_button_panel_pin_div, #rm_button_panel_pin_div')
      .each(function () {
        const $container = $(this);
        const $checkbox = $container.find('input[type="checkbox"]');
        if ($checkbox.attr('aria-label')) return;
        const title = $container.attr('title') || 'Pin Panel';
        $checkbox.attr('aria-label', title);
        $container
          .find('.right_menu_button')
          .attr({ 'aria-hidden': 'true', tabindex: '-1' })
          .removeAttr('role');
      });

    // 处理各种工具栏上的图标按钮，使用其 title 生成 aria-label
    const charPanelSelectors =
      '#rm_button_bar .menu_button, #rm_button_bar .right_menu_button, #HotSwapWrapper .hotswap, #rm_button_characters';
    $root
      .find(charPanelSelectors)
      .addBack(charPanelSelectors)
      .each(function () {
        const $btn = $(this);
        if ($btn.attr('aria-label')) return;
        const title =
          $btn.attr('title') || $btn.attr('data-i18n-title') || $btn.attr('data-original-title');
        if (title) $btn.attr('aria-label', title.split('\n')[0].trim());
      });

    // 角色排序下拉框
    const $sortOrder = $root.find('#character_sort_order').addBack('#character_sort_order');
    if ($sortOrder.length && !$sortOrder.attr('aria-label')) {
      $sortOrder.attr('aria-label', $sortOrder.attr('title') || 'Sort Characters');
    }

    // 角色标签（Tag）
    $root
      .find('.rm_tag_filter .tag')
      .addBack('.rm_tag_filter .tag')
      .each(function () {
        const $tag = $(this);
        if ($tag.attr('aria-label')) return;
        const title = $tag.find('.tag_name').attr('title');
        if (title) $tag.attr('aria-label', title);
      });

    // 用户头像选择器相关按钮
    $root
      .find('#avatar_controls .menu_button')
      .addBack('#avatar_controls .menu_button')
      .each(function () {
        const $btn = $(this);
        if ($btn.attr('aria-label')) return;
        const title = $btn.attr('title') || $btn.attr('data-i18n-title');
        if (title) $btn.attr('aria-label', title.split('\n')[0].trim());
      });

    // 世界设定（World Info/Lorebook）选择框的绑定
    $root
      .find('.character_world_info_selector, .chat_world_info_selector')
      .addBack('.character_world_info_selector, .chat_world_info_selector')
      .each(function () {
        const $el = $(this);
        if ($el.attr('aria-labelledby')) return;
        const $container = $el.closest('.range-block');
        const $label = $container.find('.range-block-title h3, .range-block-title h4').first();
        if ($label.length) {
          const labelId = $label.attr('id') || 'lbl-wi-' + Math.random().toString(36).substr(2, 5);
          $label.attr('id', labelId);
          $el.attr('aria-labelledby', labelId);
        }
      });

    // 额外世界设定的绑定，同时为 select2 内部搜索框添加标签
    $root
      .find('.character_extra_world_info_selector')
      .addBack('.character_extra_world_info_selector')
      .each(function () {
        const $el = $(this);
        if ($el.attr('aria-labelledby')) return;
        const $container = $el.closest('.range-block');
        const $label = $container.find('h4').first();
        if ($label.length) {
          const labelId =
            $label.attr('id') || 'lbl-wi-extra-' + Math.random().toString(36).substr(2, 5);
          $label.attr('id', labelId);
          $el.attr('aria-labelledby', labelId);
          const $s2Search = $container.find('.select2-search__field');
          if ($s2Search.length)
            $s2Search
              .attr('aria-labelledby', labelId)
              .attr('placeholder', 'Search additional lorebooks...');
        }
      });
  },

  // 给特定列表项注入用于无障碍排序的焦点按钮，并处理其内部操作按键的aria-label
  sortingAndLists: (root) => {
    const $root = $(root);

    // Prompt Manager 中的提示词列表
    $root
      .find('.completion_prompt_manager_prompt')
      .addBack('.completion_prompt_manager_prompt')
      .each(function () {
        const $li = $(this);
        const $controls = $li.find('.prompt_manager_prompt_controls');
        // 如果还没有注入排序按钮，则动态注入一个图标
        if ($controls.length && $controls.find('.a11y-sort-button').length === 0) {
          const itemName =
            $li.find('.completion_prompt_manager_prompt_name').text().trim() || 'Prompt';
          $li
            .find('.prompt-manager-inspect-action')
            .attr({ role: 'button', tabindex: '0', 'aria-label': 'Inspect: ' + itemName });
          const $sortBtn = $('<span>', {
            class: 'a11y-sort-button fa-solid fa-sort fa-xs',
            role: 'button',
            tabindex: '0',
            title: t`Sort`,
            'aria-label': t`Sort Prompt: ${itemName}`,
          });
          $controls.prepend($sortBtn);

          // 处理编辑、删除、开关等按钮的名称
          $controls
            .find('span')
            .not('.a11y-sort-button')
            .each(function () {
              const $btn = $(this);
              const isAction =
                $btn.hasClass('prompt-manager-toggle-action') ||
                $btn.hasClass('prompt-manager-edit-action') ||
                $btn.hasClass('prompt-manager-detach-action') ||
                $btn.hasClass('prompt-manager-delete-action');
              if (isAction) {
                $btn.attr({
                  role: 'button',
                  tabindex: '0',
                  'aria-label': `${$btn.attr('title') || 'Action'}: ${itemName}`,
                });
                if ($btn.hasClass('prompt-manager-toggle-action')) {
                  $btn.attr('aria-pressed', $btn.hasClass('fa-toggle-on') ? 'true' : 'false');
                }
              } else {
                $btn.attr('aria-hidden', 'true');
              }
            });
        }
      });

    // 快速回复 (Quick Replies) 的全局、聊天、角色预设区域
    const qrContainers = [
      { id: '#qr--global', label: 'Global' },
      { id: '#qr--chat', label: 'Chat' },
      { id: '#qr--character', label: 'Character' },
    ];
    qrContainers.forEach((container) => {
      const $cont = $root.find(container.id).addBack(container.id);
      if (!$cont.length) return;
      const titleId = `lbl-${container.id.substring(1)}-title`;
      $cont.find('.qr--title').attr('id', titleId);
      $cont.find('.qr--setListAdd').attr('aria-label', `Add new ${container.label} set`);

      $cont.find('.qr--item').each(function () {
        const $li = $(this);
        if ($li.find('.a11y-sort-button').length === 0) {
          const $select = $li.find('.qr--set');
          $select.removeAttr('aria-labelledby aria-describedby').attr('aria-labelledby', titleId);
          const setName = $select.find('option:selected').text() || 'Set';
          $li.find('.qr--visible input').attr('aria-label', `Show buttons for set: ${setName}`);
          $li.find('.fa-pencil').parent().attr('aria-label', `Edit set: ${setName}`);
          $li.find('.qr--del').attr('aria-label', `Remove set: ${setName}`);

          // 动态注入排序按钮
          const $sortBtn = $('<div>', {
            class: 'a11y-sort-button menu_button menu_button_icon fa-solid fa-sort interactable',
            role: 'button',
            tabindex: '0',
            title: t`Sort`,
            'aria-label': t`Sort set: ${setName}`,
          });
          const $delBtn = $li.find('.qr--del');
          if ($delBtn.length) $sortBtn.insertBefore($delBtn);
          else $li.append($sortBtn);
        }
      });
    });

    // 正则表达式脚本列表处理
    $root
      .find('.regex-script-label')
      .addBack('.regex-script-label')
      .each(function () {
        const $row = $(this);
        if (!$row.attr('tabindex')) $row.attr({ role: 'listitem', tabindex: '0' });
        const $btnContainer = $row.find('.regex_script_buttons');
        if ($btnContainer.length && $row.find('.a11y-sort-button').length === 0) {
          const $sortBtn = $('<div>', {
            class: 'a11y-sort-button menu_button interactable',
            role: 'button',
            tabindex: '0',
            title: t`Sort`,
            'aria-label': t`Sort Script`,
          }).append('<i class="fa-solid fa-sort"></i>');
          $btnContainer.prepend($sortBtn);
          const scriptName = $row.find('.regex_script_name').text() || 'Script';
          const $lbl = $row.find('label.checkbox');
          const $inp = $lbl.find('input');
          $lbl
            .removeAttr('for')
            .attr({
              role: 'checkbox',
              tabindex: '0',
              'aria-label': $inp.hasClass('disable_regex')
                ? `Enable script: ${scriptName}`
                : `Toggle ${scriptName}`,
              'aria-checked': $inp.prop('checked') ? 'true' : 'false',
            });
        }
      });
  },

  // 斜杠命令自动补全下拉框 (Auto Complete) 的 ARIA 交互支持
  autoComplete: (root) => {
    const $input = $('#send_textarea');
    const $visibleList = $('.autoComplete:visible');
    const $visibleDetails = $('.autoComplete-detailsWrap:visible');

    // 如果补全详情面板可见，绑定 aria-describedby
    if ($visibleDetails.length && $visibleDetails.css('opacity') !== '0') {
      const detailsId = 'a11y-slash-details';
      if ($visibleDetails.attr('id') !== detailsId) {
        $visibleDetails.attr({
          id: detailsId,
          role: 'status',
          'aria-live': 'polite',
          'aria-atomic': 'true',
        });
      }
      $visibleDetails.find('.source').each(function () {
        const $icon = $(this);
        if (!$icon.attr('aria-label')) {
          const titleText = $icon.attr('title') || 'Command Source';
          const cleanLabel = titleText.replace(/\n/g, ' ').trim();
          $icon.attr({ role: 'img', 'aria-label': cleanLabel });
        }
      });
      let currentDescribedBy = $input.attr('aria-describedby') || '';
      if (!currentDescribedBy.includes(detailsId)) {
        $input.attr('aria-describedby', (currentDescribedBy + ' ' + detailsId).trim());
      }
      $input.removeAttr('aria-activedescendant');
      $input.attr('aria-expanded', 'true');
      return;
    }

    // 如果补全列表可见，使用 aria-activedescendant 传达当前选中的项目
    if ($visibleList.length) {
      const listId = 'a11y-autocomplete-list';
      if (!$visibleList.attr('role')) {
        $visibleList.attr({ role: 'listbox', id: listId, 'aria-label': 'Command Suggestions' });
      }
      if ($input.attr('aria-expanded') !== 'true') {
        $input.attr({
          'aria-expanded': 'true',
          'aria-autocomplete': 'list',
          'aria-controls': listId,
          'aria-haspopup': 'listbox',
        });
      }
      let currentDescribedBy = $input.attr('aria-describedby') || '';
      if (currentDescribedBy.includes('a11y-slash-details')) {
        $input.attr(
          'aria-describedby',
          currentDescribedBy.replace('a11y-slash-details', '').trim(),
        );
      }
      const $items = $visibleList.find('li');
      let activeId = '';
      $items.each(function (index) {
        const $li = $(this);
        let id = $li.attr('id');
        if (!id) {
          id = `autocomplete-item-${index}`;
          $li.attr('id', id);
        }
        const isBlank = $li.hasClass('blank');
        $li.attr({
          role: 'option',
          'aria-setsize': $items.length,
          'aria-posinset': index + 1,
          'aria-disabled': isBlank ? 'true' : 'false',
        });
        // 标记高亮选中项
        if ($li.hasClass('selected')) {
          $li.attr('aria-selected', 'true');
          activeId = id;
        } else {
          $li.attr('aria-selected', 'false');
        }
      });

      if (activeId) {
        $input.attr('aria-activedescendant', activeId);
      } else {
        $input.removeAttr('aria-activedescendant');
      }
      return;
    }

    // 如果补全窗口被关闭，重置输入框的 ARIA 属性
    if ($input.attr('aria-expanded') === 'true') {
      $input
        .attr('aria-expanded', 'false')
        .removeAttr('aria-activedescendant')
        .removeAttr('aria-controls');
      let currentDescribedBy = $input.attr('aria-describedby') || '';
      if (currentDescribedBy.includes('a11y-slash-details')) {
        $input.attr(
          'aria-describedby',
          currentDescribedBy.replace('a11y-slash-details', '').trim(),
        );
      }
    }
  },

  // 为分页组件 (Pagination) 提供无障碍文本 (上一页，下一页，页码等)
  pagination: (root) => {
    const $root = $(root);
    $root
      .find('.paginationjs-pages li')
      .addBack('.paginationjs-pages li')
      .each(function () {
        const $li = $(this);
        const $a = $li.find('a');
        if (!$a.length) return;
        const text = $a.text().trim();
        let label = text;
        if (text === '«') label = t`First Page`;
        else if (text === '»') label = t`Last Page`;
        else if (text === '‹' || text === '<') label = t`Previous Page`;
        else if (text === '›' || text === '>') label = t`Next Page`;
        else if (!isNaN(parseInt(text, 10))) label = t`Page ${text}`;

        $a.attr('aria-label', label);
        if ($li.hasClass('disabled')) {
          $a.attr('aria-disabled', 'true');
          $a.removeAttr('tabindex');
        } else if ($li.hasClass('active')) {
          $a.attr('aria-current', 'page');
        } else {
          $a.attr('aria-disabled', 'false');
        }
      });
  },

  // 兜底逻辑：如果元素有 title 但没有 aria-label，自动将 title 的第一行转为 aria-label
  titlesToLabels: (root) => {
    const $root = $(root);
    $root
      .find('[title]:not([aria-label])')
      .addBack('[title]:not([aria-label])')
      .each(function () {
        const $el = $(this);
        const title = $el.attr('title') || $el.attr('data-i18n-title');
        if (title) {
          $el.attr('aria-label', title.split('\n')[0].trim());
        }
      });
  },

  // 将复杂弹出窗口、悬浮窗和菜单赋予 dialog 或 menu 的角色定义
  popupsAndMenus: (root) => {
    const $root = $(root);

    // 扩展模块容器 (Extensions)
    const $extBlocks = $root
      .find('.extension_block, .extension_container')
      .addBack('.extension_block, .extension_container')
      .filter(':visible');
    if ($extBlocks.length) {
      $extBlocks.each(function () {
        const $block = $(this);
        if ($block.attr('data-a11y-processed-ext')) return; // 防止重复处理
        const name = $block.find('.extension_name').text().trim() || $block.attr('data-name');
        $block.find('.extension_toggle input').attr('aria-label', `Enable extension: ${name}`);
        $block.find('.extension_actions button, .extension_actions .menu_button').each(function () {
          const $btn = $(this);
          const title = $btn.attr('title') || $btn.text().trim() || 'Action';
          const cleanTitle = title.split('\n')[0].trim();
          $btn.attr('aria-label', `${cleanTitle} for ${name}`);
        });
        $block.attr('data-a11y-processed-ext', 'true');
      });
    }
    const $extToolbar = $root.find('.extensions_toolbar').addBack('.extensions_toolbar');
    if ($extToolbar.length) {
      $extToolbar.attr('role', 'toolbar').attr('aria-label', 'Extensions Management Toolbar');
    }

    // 作者笔记 (Author's Note) 面板
    const $anPanel = $('#floatingPrompt');
    if ($anPanel.length && !$anPanel.attr('role')) {
      $anPanel.attr({ role: 'dialog', 'aria-label': "Author's Note Configuration" });
      $('#ANClose').attr({ role: 'button', tabindex: '0', 'aria-label': "Close Author's Note" });
      $('#floatingPromptMaximize').attr({
        role: 'button',
        tabindex: '0',
        'aria-label': "Maximize Author's Note",
      });
      $('#floatingPromptheader').attr('aria-hidden', 'true');
    }

    // CFG 调整面板
    const $cfgPanel = $('#cfgConfig');
    if ($cfgPanel.length && !$cfgPanel.attr('role')) {
      $cfgPanel.attr({ role: 'dialog', 'aria-label': 'CFG Configuration' });
      $('#CFGClose').attr({ role: 'button', tabindex: '0', 'aria-label': 'Close CFG Config' });
      $('#cfgConfigMaximize').attr({
        role: 'button',
        tabindex: '0',
        'aria-label': 'Maximize CFG Config',
      });
    }

    // Logprobs（词汇概率）查看面板
    const $logprobsPanel = $('#logprobsViewer');
    if ($logprobsPanel.length && !$logprobsPanel.attr('role')) {
      $logprobsPanel.attr({ role: 'dialog', 'aria-label': 'Token Probabilities' });
      $('#logprobsViewerClose').attr({
        role: 'button',
        tabindex: '0',
        'aria-label': 'Close Logprobs',
      });
      $('#logprobsMaximizeToggle').attr({
        role: 'button',
        tabindex: '0',
        'aria-label': 'Maximize Logprobs',
      });
      $('#logprovsViewerBlockToggle').attr({
        role: 'button',
        tabindex: '0',
        'aria-label': 'Toggle Logprobs View',
      });
      $('#logprobsReroll').attr({ role: 'button', tabindex: '0' });
    }

    // 历史聊天选择窗口
    const $selectChat = $('#select_chat_popup');
    if ($selectChat.length) {
      if (!$selectChat.attr('role')) {
        $selectChat.attr({ role: 'dialog', 'aria-label': 'Chat History' });
        $('#select_chat_cross').attr({
          role: 'button',
          tabindex: '0',
          'aria-label': 'Close Chat History',
        });
        $('#newChatFromManageScreenButton, #chat_import_button').attr({
          role: 'button',
          tabindex: '0',
        });
        $selectChat.find('.chatBackupsList').attr('role', 'list');
      }
      $selectChat.find('.select_chat_block').each(function () {
        const $el = $(this);
        if (!$el.attr('tabindex')) {
          $el.attr({ role: 'button', tabindex: '0' });
          $el
            .find('.renameChatButton, .exportRawChatButton, .exportChatButton, .PastChat_cross')
            .attr({ role: 'button', tabindex: '0' });
        }
      });
    }

    // 通用动作按钮模态框
    const $actionModal = $root.find('.actionButtonsModal').addBack('.actionButtonsModal');
    if ($actionModal.length) {
      $actionModal.attr('role', 'menu');
      $actionModal.find('.actionButton').attr({ role: 'menuitem', tabindex: '0' });
    }

    // Data Bank (附件数据银行)
    const $dataBank = $root.find('.dataBankAttachments').addBack('.dataBankAttachments');
    if ($dataBank.length) {
      $dataBank.closest('dialog').attr('aria-label', 'Data Bank');
      $dataBank.find('.attachmentSort').attr('aria-label', 'Sort attachments');
      $dataBank
        .find(
          '.bulkActionSelectAll, .bulkActionSelectNone, .bulkActionDisable, .bulkActionEnable, .bulkActionDelete, .openActionModalButton',
        )
        .attr({ role: 'button', tabindex: '0' });
    }

    // 导出格式弹出层
    const $exportFormatPopup = $('#export_format_popup');
    if ($exportFormatPopup.length && !$exportFormatPopup.attr('role')) {
      $exportFormatPopup.attr({ role: 'menu', 'aria-label': 'Export Format Options' });
      $exportFormatPopup.find('.export_format').attr({
        role: 'menuitem',
        tabindex: '0',
        'aria-label': function () {
          return $(this).text().trim() + ' format';
        },
      });
    }

    // 角色互联(Persona Connections)弹出层
    const $personaPopup = $root
      .closest('.popup')
      .filter((_, el) => $(el).find('h3:contains("Persona Connections")').length > 0);
    if (
      $personaPopup.length &&
      $personaPopup.is(':visible') &&
      !$personaPopup.find('.persona-list').attr('role')
    ) {
      const $list = $personaPopup.find('.persona-list');
      $list.attr({ role: 'list', 'aria-label': 'Connected Personas List' });
      $list.find('.avatar').attr({ role: 'listitem', tabindex: '0' });
    }

    // 替代开场白 (Alternate Greetings) 列表
    const $altGreetings = $root
      .find('.alternate_greetings_list')
      .addBack('.alternate_greetings_list');
    if ($altGreetings.length && $altGreetings.is(':visible')) {
      $altGreetings.attr('role', 'list');
      $root
        .find('.add_alternate_greeting')
        .attr({ role: 'button', tabindex: '0', 'aria-label': 'Add new greeting' });
      $altGreetings.find('.alternate_greeting').each(function () {
        const $item = $(this);
        if ($item.attr('role')) return;
        $item.attr('role', 'listitem');
        $item.find('.move_up_alternate_greeting').attr('aria-label', 'Move greeting up');
        $item.find('.move_down_alternate_greeting').attr('aria-label', 'Move greeting down');
        $item.find('.delete_alternate_greeting').attr('aria-label', 'Delete greeting');
        const index = $item.data('index');
        const labelId = 'lbl-alt-greet-' + index;
        $item.find('strong span').first().closest('strong').attr('id', labelId);
        $item.find('textarea').attr('aria-labelledby', labelId);
      });
    }

    // 顶部用户选项设置菜单 (#options_button)
    const $optionsBtn = $('#options_button');
    if ($optionsBtn.length && !$optionsBtn.attr('aria-haspopup')) {
      $optionsBtn.attr({
        role: 'button',
        tabindex: '0',
        'aria-haspopup': 'true',
        'aria-label': 'User Options',
      });
      $('#options .options-content').attr({ role: 'menu', 'aria-labelledby': 'options_button' });
      $('#options a').attr({ role: 'menuitem', tabindex: '0' });
    }
  },
};

/**
 * 集中执行所有的特制无障碍处理器
 * @param {Element|Document|Node} [rootElement=document]
 */
const enhanceSpecificA11y = (rootElement = document) => {
  if (!isA11yEnabled) return;
  Object.values(SpecificProcessors).forEach((process) => {
    try {
      process(rootElement);
    } catch (e) {
      console.warn('[A11y] Processor error:', e);
    }
  });
};

/**
 * 在关闭无障碍模式时，清理注入到 DOM 中的所有无障碍属性和节点
 */
function cleanupA11y() {
  if (mainObserver) {
    mainObserver.disconnect();
    mainObserver = null;
  }
  $(`[${GENERIC_ATTR}]`).removeAttr(GENERIC_ATTR);
  $(
    '[role="button"], [role="list"], [role="listitem"], [role="toolbar"], [role="tablist"], [role="tab"], [role="status"]',
  ).removeAttr(
    'role tabindex aria-label aria-hidden aria-expanded aria-controls aria-pressed aria-valuemin aria-valuemax aria-describedby aria-labelledby aria-haspopup aria-checked aria-level',
  );
  $('.a11y-sort-button').remove();
  $('.a11y-refactored').removeClass('a11y-refactored');
  $('[id^="st-a11y-"]').removeAttr('id');
  $('[id^="drawer-"]').removeAttr('id');
  $('[id^="label-st-a11y-"]').removeAttr('id');
}

/**
 * 启用或禁用无障碍核心系统，并设置监听DOM变化的 MutationObserver
 * @param {boolean} enabled
 */
export function setAccessibilityEnabled(enabled) {
  if (isA11yEnabled === enabled && mainObserver) return;
  isA11yEnabled = enabled;

  if (!enabled) {
    cleanupA11y();
    return;
  }

  // 首次运行应用规则
  applyGenericA11yRules(document.body);
  enhanceSpecificA11y(document.body);

  // 隐藏传统的不可访问发送按钮，避免焦点重叠
  $('#send_but').attr({ tabindex: '-1', 'aria-hidden': 'true' });

  // 初始化全局 DOM 观察器，确保后续通过 JS 插入的新元素也能自动挂载无障碍属性
  mainObserver = new MutationObserver((mutations) => {
    if (!isA11yEnabled) return;
    for (const mutation of mutations) {
      if (mutation.type === 'childList') {
        if (mutation.addedNodes.length > 0) {
          mutation.addedNodes.forEach((node) => {
            if (node instanceof Element) {
              applyGenericA11yRules(node);
              enhanceSpecificA11y(node);
            }
          });
        }
      }
    }
    // 当 AI 生成时保护中断按钮的焦点（目前内部为空，保留扩展性）
    if (isAiGenerating) {
      const stopBtn = document.getElementById('mes_stop');
      if (stopBtn && document.activeElement !== stopBtn && stopBtn.offsetParent !== null) {
      }
    }
  });

  mainObserver.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['style', 'class', 'hidden', 'open', 'aria-hidden'],
  });
}

/**
 * 系统初始化主入口：绑定全局无障碍快捷键、键盘事件及 SillyTavern 内核流事件
 */
export function initAccessibility() {
  $('#send_but').attr({ tabindex: '-1', 'aria-hidden': 'true' });

  // 当排序按钮获得焦点时，播报当前项目在列表中的编号
  $(document).on('focus', '.a11y-sort-button', function () {
    if (!isA11yEnabled) return;
    const $this = $(this);
    const validSelectors =
      '.qr--item, .qr--set-item, .completion_prompt_manager_prompt, .regex-script-label, .list-group-item';
    const $item = $this.closest(validSelectors);
    if ($item.length) {
      const $container = $item.parent();
      const $siblings = $container.children(validSelectors);
      const idx = $siblings.index($item) + 1;
      const tot = $siblings.length;
      console.log(
        `[A11y Debug] Item Focus: ${idx}/${tot} (Total DOM children: ${$container.children().length})`,
      );
      announceA11y(t`Position ${idx} of ${tot}.`);
    }
  });

  // 点击或按 Enter/Space 激活排序按钮时，根据当前容器类型调起排序菜单
  $(document).on('click keydown', '.a11y-sort-button', function (e) {
    if (!isA11yEnabled) return;
    if (e.type === 'keydown' && e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    e.stopPropagation();
    if ($(this).closest('.completion_prompt_manager_prompt').length) {
      handleSortMenu(this, '.completion_prompt_manager_prompt', '#completion_prompt_manager_list');
    } else if ($(this).closest('.qr--item').length) {
      const containerId = $(this).closest('.qr--setList').attr('id');
      handleSortMenu(this, '.qr--item', '#' + containerId);
    } else if ($(this).closest('.qr--set-item').length) {
      handleSortMenu(this, '.qr--set-item', '.qr--set-qrListContents');
    } else if ($(this).closest('.regex-script-label').length) {
      handleSortMenu(this, '.regex-script-label', '.regex-script-container');
    }
  });

  // 维护正则表达式脚本复选框的 aria-checked 状态
  $(document).on('change', '.regex-script-container input[type="checkbox"]', function () {
    if (!isA11yEnabled) return;
    const $lbl = $(this).closest('.regex-script-label').find('label.checkbox');
    if ($lbl.length) {
      $lbl.attr('aria-checked', $(this).prop('checked') ? 'true' : 'false');
    }
  });

  // 播报左右面板锁定/解锁的语音提示
  $(document).on('change', '#lm_button_panel_pin, #rm_button_panel_pin', function () {
    if (!isA11yEnabled) return;
    const $checkbox = $(this);
    const isChecked = $checkbox.prop('checked');
    const panelName =
      $checkbox.attr('id') === 'lm_button_panel_pin'
        ? t`AI Configuration`
        : t`Character Management`;
    const status = isChecked ? t`Locked open` : t`Unlocked`;
    announceA11y(`${panelName} panel ${status}`);
  });

  // 提示词管理界面开关按钮状态更新
  $(document).on('click', '.prompt-manager-toggle-action', function () {
    if (!isA11yEnabled) return;
    setTimeout(() => {
      $(this).attr('aria-pressed', $(this).hasClass('fa-toggle-on') ? 'true' : 'false');
    }, 50);
  });

  // 辅助函数：在聊天记录间通过键盘上下键平滑切换焦点
  const moveMessageFocus = ($current, $target) => {
    if ($target.length) {
      $current.attr('tabindex', '-1');
      $target.attr('tabindex', '0').trigger('focus');
      $target[0].scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  };

  // 聊天界面的按键劫持：支持使用上下箭头在消息气泡间跳转，按 Esc 返回输入框
  $(document).on('keydown', '#chat .mes', function (e) {
    if (!isA11yEnabled) return;
    if (e.target !== this) return; // 只处理消息气泡本身的焦点
    const $this = $(this);
    const $allMessages = $('#chat .mes:visible');
    const index = $allMessages.index($this);
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        if (index < $allMessages.length - 1) {
          moveMessageFocus($this, $allMessages.eq(index + 1));
        }
        break;
      case 'ArrowUp':
        e.preventDefault();
        if (index > 0) {
          moveMessageFocus($this, $allMessages.eq(index - 1));
        }
        break;
      case 'Escape':
        e.preventDefault();
        $('#send_textarea').trigger('focus');
        announceA11y(t`Returned to text input`);
        break;
    }
  });

  // 使部分标记为 role="button" 的元素可以通过回车和空格键被"点击"
  $(document).on(
    'keydown',
    '[role="button"][tabindex="0"]:not(.a11y-sort-button), [role="listitem"][tabindex="0"], [role="menuitem"][tabindex="0"], .prompt-manager-toggle-action, .killSwitch, .inline-drawer-toggle, #options_button, #extensionsMenuButton',
    function (e) {
      if (!isA11yEnabled) return;
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        if (this.tagName !== 'BUTTON') {
          $(this).trigger('click');
        }
      }
    },
  );

  // 在扩展菜单抽屉内按 Esc，会关闭当前抽屉并将焦点交还给抽屉头部
  $(document).on('keydown', '.extension_container .inline-drawer', function (e) {
    if (!isA11yEnabled) return;
    if (e.key === 'Escape') {
      const $drawer = $(this);
      const $content = $drawer.find('.inline-drawer-content');
      if ($content.is(':visible')) {
        e.preventDefault();
        e.stopPropagation();
        const $header = $drawer.find('.inline-drawer-toggle');
        $header.trigger('click');
        $header.trigger('focus');
        announceA11y(t`Extension menu closed.`);
      }
    }
  });

  // 全局 Esc 键处理：负责关闭各种顶部菜单、弹出面板和选项界面
  $(document).on('keydown', function (e) {
    if (!isA11yEnabled || e.key !== 'Escape') return;

    // 关闭顶部选项菜单
    const $optionsMenu = $('#options');
    if ($optionsMenu.is(':visible') && $optionsMenu.find('.options-content').is(':visible')) {
      if ($(e.target).closest('#options').length) {
        e.preventDefault();
        e.stopPropagation();
        $('#options_button').trigger('click').trigger('focus');
        return;
      }
    }

    // 关闭顶部扩展列表
    const $extMenu = $('#extensionsMenu');
    if ($extMenu.is(':visible')) {
      if (
        $(e.target).closest('#extensionsMenu').length &&
        !$(e.target).closest('.inline-drawer-content').length
      ) {
        e.preventDefault();
        e.stopPropagation();
        $('#extensionsMenuButton').trigger('click').trigger('focus');
        return;
      }
    }

    // 关闭聊天选择窗口
    const $selectChat = $('#select_chat_popup');
    if ($selectChat.is(':visible')) {
      e.preventDefault();
      e.stopPropagation();
      $('#select_chat_cross').trigger('click');
      return;
    }

    // 关闭各种悬浮窗口面板
    const panels = [
      { id: '#floatingPrompt', closeBtn: '#ANClose' },
      { id: '#cfgConfig', closeBtn: '#CFGClose' },
      { id: '#logprobsViewer', closeBtn: '#logprobsViewerClose' },
    ];
    for (let panel of panels) {
      const $p = $(panel.id);
      if ($p.is(':visible') && $p.css('opacity') !== '0') {
        e.preventDefault();
        e.stopPropagation();
        $(panel.closeBtn).trigger('click');
        return;
      }
    }

    // 关闭导出格式选项面板
    const $exportFormat = $('#export_format_popup');
    if ($exportFormat.is(':visible')) {
      e.preventDefault();
      e.stopPropagation();
      $exportFormat.hide();
      return;
    }

    // 关闭移动端的消息附加按钮菜单
    const $activeExtraButtons = $('.extraMesButtons.mobile-active');
    if ($activeExtraButtons.length) {
      e.preventDefault();
      e.stopPropagation();
      $activeExtraButtons.removeClass('mobile-active');
      $activeExtraButtons.closest('.mes').trigger('focus');
      announceA11y(t`Message actions closed.`);
      return;
    }
  });

  // 监听选项菜单和扩展菜单的点击事件，打开时焦点自动跳入弹窗中的第一个可聚焦元素
  $(document).on('click', '#options_button', function () {
    if (!isA11yEnabled) return;
    setTimeout(() => {
      const $menu = $('#options');
      if ($menu.is(':visible') && $menu.find('.options-content').is(':visible')) {
        $menu.find('[tabindex="0"]:visible').first().trigger('focus');
      }
    }, 50);
  });

  $(document).on('click', '#extensionsMenuButton', function () {
    if (!isA11yEnabled) return;
    setTimeout(() => {
      const $menu = $('#extensionsMenu');
      if ($menu.is(':visible')) {
        $menu.find('[tabindex="0"]:visible').first().trigger('focus');
      }
    }, 50);
  });

  // 监听 SillyTavern 内核流事件：开始生成消息时，播报并尝试将焦点转移到停止按钮
  eventSource.on(event_types.GENERATION_STARTED, (context) => {
    if (!isA11yEnabled) return;
    const typeStr = typeof context === 'string' ? context : context?.type || 'normal';
    const isQuiet = typeof context === 'string' ? context === 'quiet' : context?.quiet || false;
    // 静默请求（如自动总结等）不需要打断用户
    if (isQuiet || typeStr === 'quiet' || typeStr === 'summarize' || typeStr === 'classify') return;

    if (!isAiGenerating) {
      isAiGenerating = true;
      announceA11y(t`AI is generating response...`);
      setTimeout(() => {
        const stopBtn = document.getElementById('mes_stop');
        if (stopBtn && stopBtn.offsetParent !== null) stopBtn.focus();
      }, 50);
    }
  });

  // 监听 SillyTavern 内核流事件：AI 回复完成时播报回复内容，将焦点强制切回聊天输入框
  eventSource.on(event_types.CHARACTER_MESSAGE_RENDERED, (mid) => {
    isAiGenerating = false;
    const msg = chat[mid];
    if (msg) announceA11y(t`AI has replied: ${msg.mes}`);

    // 重置聊天气泡的 tabindex
    $('#chat .mes').attr('tabindex', '-1');
    $('#chat .mes').last().attr('tabindex', '0');

    // 聚焦输入框
    $('#send_textarea').trigger('focus');
  });

  // 监听 SillyTavern 内核流事件：生成被中止时播报
  eventSource.on(event_types.GENERATION_STOPPED, () => {
    isAiGenerating = false;
    announceA11y(t`AI generation stopped.`);
    $('#send_textarea').trigger('focus');
  });

  // 防止用户在聊天保存或编辑时误关闭页面导致数据丢失
  window.addEventListener('beforeunload', (e) => {
    if (isChatSaving || (typeof this_edit_mes_id === 'number' && this_edit_mes_id >= 0)) {
      e.preventDefault();
      e.returnValue = true;
    }
  });

  // 快速回复内的多行文本模态框：按 Esc 时将焦点退出编辑框并回到模态框底部的确定按钮
  $(document).on('keydown', '#qr--modal-message', function (e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      const $popup = $(this).closest('.popup-body');
      const $focusable = $popup
        .find('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')
        .filter(':visible:not(:disabled)');
      const index = $focusable.index(this);

      // 尝试焦点下移，如果已经是最后一个焦点元素则直接跳到 OK 按钮
      if (index > -1 && index < $focusable.length - 1) {
        $focusable.eq(index + 1).trigger('focus');
      } else {
        $popup.find('.popup-button-ok').trigger('focus');
      }
      announceA11y(t`Exited text editor.`);
    }
  });

  // 设置页面级的 ARIA Landmark (界标)，方便屏幕阅读器按区域（如 Banner、Region、Main）快速跳转
  const setupLandmarks = () => {
    $('#top-settings-holder').attr({ role: 'banner', 'aria-label': 'Main Navigation' });
    $('#left-nav-panel').attr({ role: 'region', 'aria-label': 'AI Configuration' });
    $('#right-nav-panel').attr({ role: 'region', 'aria-label': 'Character Management' });
    $('#sheld').attr({ role: 'main', 'aria-label': 'Chat Log' });
    $('#send_form').attr({ role: 'form', 'aria-label': 'Message Input' });
  };
  setupLandmarks();

  // 全局焦点重置规则：从侧边栏等抽屉面板按 Esc 也能快速回到主聊天框
  $(document).on('keydown', function (e) {
    if (!isA11yEnabled || e.key !== 'Escape') return;
    if ($(e.target).is('#send_textarea')) return;
    if ($(e.target).closest('.drawer-content, .options-content').length) {
      e.preventDefault();
      $('#send_textarea').trigger('focus');
      announceA11y(t`Focus moved to Chat Input`);
    }
  });

  // 初始化时全量刷一遍无障碍规则
  applyGenericA11yRules(document.body);
  enhanceSpecificA11y();
}
