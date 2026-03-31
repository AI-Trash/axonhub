import * as m from '@/paraglide/messages';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';

import { graphqlRequest } from '@/gql/graphql';
import { useErrorHandler } from '@/hooks/use-error-handler';

import {
  dataStorageSchema,
  dataStoragesConnectionSchema,
  createDataStorageInputSchema,
  updateDataStorageInputSchema,
  dataStorageWithCredentialsSchema,
  type DataStorage,
  type DataStoragesConnection,
  type CreateDataStorageInput,
  type UpdateDataStorageInput,
} from './schema';

export type { DataStorage, DataStoragesConnection, CreateDataStorageInput, UpdateDataStorageInput };

// GraphQL queries and mutations
const DATA_STORAGES_QUERY = `
  query DataStorages(
    $first: Int
    $after: Cursor
    $where: DataStorageWhereInput
    $orderBy: DataStorageOrder
  ) {
    dataStorages(
      first: $first
      after: $after
      where: $where
      orderBy: $orderBy
    ) {
      edges {
        node {
          id
          name
          description
          type
          primary
          status
          settings {
            directory
            s3 {
              bucketName
              endpoint
              region
              pathStyle
            }
            gcs {
              bucketName
            }
          }
          createdAt
          updatedAt
        }
      }
      pageInfo {
        hasNextPage
        hasPreviousPage
        startCursor
        endCursor
      }
      totalCount
    }
  }
`;

const CREATE_DATA_STORAGE_MUTATION = `
  mutation CreateDataStorage($input: CreateDataStorageInput!) {
    createDataStorage(input: $input) {
      id
      name
      description
      type
      primary
      status
      settings {
        directory
        s3 {
          bucketName
          endpoint
          region
        }
        gcs {
          bucketName
        }
      }
      createdAt
      updatedAt
    }
  }
`;

const UPDATE_DATA_STORAGE_MUTATION = `
  mutation UpdateDataStorage($id: ID!, $input: UpdateDataStorageInput!) {
    updateDataStorage(id: $id, input: $input) {
      id
      name
      description
      type
      primary
      status
      settings {
        directory
        s3 {
          bucketName
          endpoint
          region
        }
        gcs {
          bucketName
        }
      }
      createdAt
      updatedAt
    }
  }
`;

const UPDATE_DATA_STORAGE_STATUS_MUTATION = `
  mutation UpdateDataStorageStatus($id: ID!, $input: UpdateDataStorageInput!) {
    updateDataStorage(id: $id, input: $input) {
      id
      status
      updatedAt
    }
  }
`;

// Hooks
export function useDataStorages(variables?: Record<string, any>) {
  const { handleError } = useErrorHandler();

  return useQuery({
    queryKey: ['dataStorages', variables],
    queryFn: async () => {
      try {
        const data = await graphqlRequest<{ dataStorages: DataStoragesConnection }>(DATA_STORAGES_QUERY, variables);
        return dataStoragesConnectionSchema.parse(data.dataStorages);
      } catch (error) {
        handleError(error, m["dataStorages.errors.fetchData"]());
        throw error;
      }
    },
  });
}

export function useArchiveDataStorage() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (id: string) => {
      const data = await graphqlRequest<{ updateDataStorage: Pick<DataStorage, 'id' | 'status'> }>(UPDATE_DATA_STORAGE_STATUS_MUTATION, {
        id,
        input: {
          status: 'archived',
        },
      });

      return dataStorageSchema.pick({ id: true, status: true }).parse(data.updateDataStorage);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['dataStorages'] });
      toast.success(m["dataStorages.messages.archiveSuccess"]());
    },
    onError: (error: any) => {
      toast.error(m["dataStorages.messages.archiveError"]());
    },
  });
}

export function useCreateDataStorage() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (input: CreateDataStorageInput) => {
      // Validate input
      const validatedInput = createDataStorageInputSchema.parse(input);
      const data = await graphqlRequest<{ createDataStorage: DataStorage }>(CREATE_DATA_STORAGE_MUTATION, {
        input: validatedInput,
      });
      // Validate response data
      return dataStorageSchema.parse(data.createDataStorage);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['dataStorages'] });
      toast.success(m["common.messages.success"]());
    },
    onError: (error: any) => {
      toast.error(m["dataStorages.errors.createError"]());
    },
  });
}

export function useUpdateDataStorage() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ id, input }: { id: string; input: UpdateDataStorageInput }) => {
      // Validate input
      const validatedInput = updateDataStorageInputSchema.parse(input);
      const data = await graphqlRequest<{ updateDataStorage: DataStorage }>(UPDATE_DATA_STORAGE_MUTATION, {
        id,
        input: validatedInput,
      });
      // Use schema that includes credentials since this is for update
      return dataStorageWithCredentialsSchema.parse(data.updateDataStorage);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['dataStorages'] });
      toast.success(m["common.messages.success"]());
    },
    onError: (error: any) => {
      toast.error(m["dataStorages.errors.updateError"]());
    },
  });
}
