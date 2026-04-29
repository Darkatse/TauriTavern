import { getSortableDelay } from '../../../utils.js';
import { isScreenReaderAssistanceEnabled } from '../../../a11y/screen-reader.js';
import { t } from '../../../i18n.js';
import { QuickReplySetLink } from './QuickReplySetLink.js';
import { QuickReplySet } from './QuickReplySet.js';

function getSortDirectionOffset(direction) {
    if (direction === 'up') return -1;
    if (direction === 'down') return 1;
    throw new Error(`Unsupported quick reply set sort direction: ${direction}`);
}

export class QuickReplyConfig {
    /**@type {QuickReplySetLink[]}*/ setList = [];
    /**@type {'global'|'chat'|'character'}*/ scope;

    /**@type {Function}*/ onUpdate;
    /**@type {Function}*/ onRequestEditSet;

    /**@type {HTMLElement}*/ dom;
    /**@type {HTMLElement}*/ setListDom;




    static from(props) {
        props.setList = props.setList?.map(it=>QuickReplySetLink.from(it))?.filter(it=>it.set) ?? [];
        const instance = Object.assign(new this(), props);
        instance.init();
        return instance;
    }




    init() {
        this.setList.forEach(it=>this.hookQuickReplyLink(it));
    }


    hasSet(qrs) {
        return this.setList.find(it=>it.set == qrs) != null;
    }
    addSet(qrs, isVisible = true) {
        if (!this.hasSet(qrs)) {
            const qrl = new QuickReplySetLink();
            qrl.set = qrs;
            qrl.isVisible = isVisible;
            this.hookQuickReplyLink(qrl);
            this.setList.push(qrl);
            this.setListDom.append(qrl.renderSettings(this.setList.length - 1));
            this.update();
        }
    }
    removeSet(qrs) {
        const idx = this.setList.findIndex(it=>it.set == qrs);
        if (idx > -1) {
            this.setList.splice(idx, 1);
            this.update();
            this.updateSetListDom();
        }
    }




    renderSettingsInto(/**@type {HTMLElement}*/root) {
        /**@type {HTMLElement}*/
        this.dom = root;
        this.setListDom = root.querySelector('.qr--setList');
        root.querySelector('.qr--setListAdd').addEventListener('click', ()=>{
            const newSet = QuickReplySet.list.find(qr=>!this.setList.find(qrl=>qrl.set == qr));
            if (newSet) {
                this.addSet(newSet);
            } else {
                toastr.warning('All existing QR Sets have already been added.');
            }
        });
        this.updateSetListDom();
    }
    updateSetListDom() {
        this.setListDom.innerHTML = '';
        // @ts-ignore
        $(this.setListDom).sortable({
            delay: getSortableDelay(),
            stop: ()=>this.onSetListSort(),
        });
        const visibleLinks = this.setList.filter(it=>!it.set.isDeleted);
        const showScreenReaderSortUi = isScreenReaderAssistanceEnabled();
        if (!showScreenReaderSortUi) {
            this.dom?.querySelector('.qr--setSortStatus')?.remove();
        }
        visibleLinks.forEach((qrl, visualIndex)=>{
            const idx = this.setList.indexOf(qrl);
            this.setListDom.append(qrl.renderSettings(idx, visibleLinks.length, showScreenReaderSortUi, visualIndex));
        });
    }


    onSetListSort() {
        this.setList = Array.from(this.setListDom.children).map((it,idx)=>{
            const qrl = this.setList[Number(it.getAttribute('data-order'))];
            qrl.index = idx;
            it.setAttribute('data-order', String(idx));
            return qrl;
        });
        this.update();
    }

    moveSetLink(index, direction) {
        if (!Number.isInteger(index) || index < 0 || index >= this.setList.length) {
            throw new Error(`Quick Reply set link index out of range: ${index}`);
        }

        const targetIndex = index + getSortDirectionOffset(direction);
        if (targetIndex < 0 || targetIndex >= this.setList.length) {
            return { index, position: index + 1, total: this.setList.length, moved: false };
        }

        this.setList.splice(targetIndex, 0, this.setList.splice(index, 1)[0]);
        this.update();
        this.updateSetListDom();
        return { index: targetIndex, position: targetIndex + 1, total: this.setList.length, moved: true };
    }

    moveSetLinkFromUi(index, direction) {
        const result = this.moveSetLink(index, direction);
        this.announceSortPosition(result.position, result.total);
        this.focusSetLinkSortControl(result.index, direction);
        return result;
    }

    announceSortPosition(position, total) {
        const status = this.ensureSortStatus();
        status.textContent = t`Moved to position ${position} of ${total}.`;
    }

    ensureSortStatus() {
        if (!this.setListDom) {
            throw new Error('Quick Reply set sort status requires a rendered set list.');
        }

        let status = this.dom?.querySelector('.qr--setSortStatus');
        if (!(status instanceof HTMLElement)) {
            status = document.createElement('div');
            status.classList.add('qr--setSortStatus', 'sr-only');
            status.setAttribute('role', 'status');
            status.setAttribute('aria-live', 'polite');
            status.setAttribute('aria-atomic', 'true');
            this.setListDom.insertAdjacentElement('afterend', status);
        }

        return status;
    }

    focusSetLinkSortControl(index, direction) {
        const link = this.setList[index];
        if (!link?.settingsDom) {
            throw new Error(`Quick Reply set link focus target not found: ${index}`);
        }

        const selector = direction === 'up' ? '.qr--moveSetUp' : '.qr--moveSetDown';
        let control = link.settingsDom.querySelector(`${selector}:not(.disabled)`);
        control ??= link.settingsDom.querySelector('.qr--moveSetUp:not(.disabled), .qr--moveSetDown:not(.disabled)');
        if (!(control instanceof HTMLElement)) {
            throw new Error(`Quick Reply set link sort control not found: ${index}`);
        }

        control.focus();
    }




    /**
     * @param {QuickReplySetLink} qrl
     */
    hookQuickReplyLink(qrl) {
        qrl.onDelete = ()=>this.deleteQuickReplyLink(qrl);
        qrl.onUpdate = ()=>this.update();
        qrl.onRequestEditSet = ()=>this.requestEditSet(qrl.set);
        qrl.onMove = direction=>this.moveSetLinkFromUi(qrl.index, direction);
    }

    deleteQuickReplyLink(qrl) {
        this.setList.splice(this.setList.indexOf(qrl), 1);
        this.update();
    }

    update() {
        if (this.onUpdate) {
            this.onUpdate(this);
        }
    }

    requestEditSet(qrs) {
        if (this.onRequestEditSet) {
            this.onRequestEditSet(qrs);
        }
    }

    toJSON() {
        return {
            setList: this.setList,
        };
    }
}
