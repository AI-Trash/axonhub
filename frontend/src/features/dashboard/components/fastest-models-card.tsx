'use client';


import { formatNumber } from '@/utils/format-number';

import { useFastestModels } from '../data/fastest-performers';
import type { FastestModel } from '../data/fastest-performers';
import { FastestPerformersCard } from './fastest-performers-card';
import * as m from '@/paraglide/messages';

export function FastestModelsCard() {
  return (
    <FastestPerformersCard<FastestModel>
      title={m["dashboard.cards.fastestPerformers.models"]()}
      description={(totalRequests) =>
        m["dashboard.cards.fastestPerformers.description"]({
          type: m["dashboard.cards.fastestPerformers.modelType"](),
          count: formatNumber(totalRequests) })
      }
      noDataLabel={m["dashboard.cards.fastestPerformers.noData"]()}
      useData={useFastestModels}
      getName={(item) => item.modelName}
    />
  );
}
