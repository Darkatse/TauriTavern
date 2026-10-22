/* Select2 4.1 backport: remove after upgrading to an upstream release with
 * field naming and non-pointer activation on the focused controls/results. */
(function ($) {
    const Single = $.fn.select2.amd.require('select2/selection/single');
    const Search = $.fn.select2.amd.require('select2/selection/search');
    const Results = $.fn.select2.amd.require('select2/results');
    const Utils = $.fn.select2.amd.require('select2/utils');

    function nameField(widget, target, valueId) {
        const source = widget.$element[0];
        const authorName = source.getAttribute('aria-label')?.trim();
        const labels = source.getAttribute('aria-labelledby') || (!authorName && Array.from(source.labels ?? [], (label, index) => {
            label.id ||= `${Utils.GetUniqueElementId(source)}_${index}_label`;
            return label.id;
        }).join(' '));
        if (labels) {
            target.attr('aria-labelledby', [labels, valueId].filter(Boolean).join(' ')).removeAttr('aria-label');
        } else if (authorName) {
            target.attr('aria-label', authorName).removeAttr('aria-labelledby');
            if (valueId) {
                target[0].id ||= `${valueId}-field`;
                target.attr('aria-labelledby', `${target[0].id} ${valueId}`);
            }
        }
        if (source.hasAttribute('aria-describedby')) target.attr('aria-describedby', source.getAttribute('aria-describedby'));
    }

    const bindSingle = Single.prototype.bind;
    Single.prototype.bind = function () {
        bindSingle.apply(this, arguments);
        nameField(this, this.$selection, this.$selection.find('.select2-selection__rendered').attr('id'));
        this.$selection.on('click', event => {
            if (event.originalEvent?.isTrusted) return; // Browser/AT activation already passed through mousedown.
            this.trigger('toggle', { originalEvent: event });
        });
    };
    const bindSearch = Search.prototype.bind;
    Search.prototype.bind = function () {
        bindSearch.apply(this, arguments);
        nameField(this, this.$search);
    };
    const bindResults = Results.prototype.bind;
    Results.prototype.bind = function () {
        bindResults.apply(this, arguments);
        this.$results.on('click', '.select2-results__option--selectable', function (event) {
            if (event.originalEvent?.isTrusted) return; // Browser/AT activation already passed through mouseup.
            $(this).trigger('mouseup');
        });
    };
})(window.jQuery);
